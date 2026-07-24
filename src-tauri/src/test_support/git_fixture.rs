use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tempfile::TempDir;
use url::Url;

use crate::application::git_transport::{GitSourceTransport, ProcessGitTransport};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{build_skill_payload, compute_cli_project_hash_from_payload};
use crate::core::{CloneProgress, CloneResult};
use crate::error::AppError;

/// 对真实 process Git 做计数和故障注入，不自行模拟 Git wire protocol。
#[derive(Debug, Default)]
pub(crate) struct CountingGitTransport {
    process: ProcessGitTransport,
    clone_count: AtomicUsize,
    reject_probes: AtomicBool,
    public_source: Option<String>,
    local_source: Option<String>,
}

impl CountingGitTransport {
    pub(crate) fn for_repo(repo: &BareSkillRepo) -> Self {
        Self {
            process: ProcessGitTransport,
            clone_count: AtomicUsize::new(0),
            reject_probes: AtomicBool::new(false),
            public_source: Some(repo.source()),
            local_source: Some(repo.local_source()),
        }
    }

    pub(crate) fn clone_count(&self) -> usize {
        self.clone_count.load(Ordering::SeqCst)
    }

    pub(crate) fn reject_probes(&self) {
        self.reject_probes.store(true, Ordering::SeqCst);
    }

    fn resolved_source<'a>(&'a self, source: &'a str) -> &'a str {
        if self.public_source.as_deref() == Some(source) {
            self.local_source.as_deref().unwrap_or(source)
        } else {
            source
        }
    }
}

impl GitSourceTransport for CountingGitTransport {
    fn clone_source(
        &self,
        url: &str,
        git_ref: Option<&str>,
        on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError> {
        self.clone_count.fetch_add(1, Ordering::SeqCst);
        self.process.clone_source(
            self.resolved_source(url),
            git_ref,
            on_progress,
            cancellation,
        )
    }

    fn probe_ref_revision(
        &self,
        url: &str,
        git_ref: Option<&str>,
        cancellation: CancellationSignal,
    ) -> Result<String, AppError> {
        if self.reject_probes.load(Ordering::SeqCst) {
            return Err(AppError::GitCloneFailed {
                message: "injected remote ref probe failure".to_string(),
            });
        }
        self.process
            .probe_ref_revision(self.resolved_source(url), git_ref, cancellation)
    }
}

pub(crate) struct BareSkillRepo {
    _root: TempDir,
    pub(crate) work: PathBuf,
    remote: PathBuf,
    revision: AtomicUsize,
}

impl BareSkillRepo {
    pub(crate) fn new(skills: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("bare repository tempdir");
        let work = root.path().join("work");
        let remote = root.path().join("remote.git");
        run_git(
            root.path(),
            &[
                "init",
                "-q",
                "-b",
                "main",
                work.to_str().expect("work path"),
            ],
        );
        run_git(&work, &["config", "user.email", "test@example.com"]);
        run_git(&work, &["config", "user.name", "Skill Deck Test"]);
        run_git(&work, &["config", "commit.gpgsign", "false"]);
        fs::write(work.join(".gitattributes"), "* text eol=lf\n*.bin -text\n")
            .expect("write fixture attributes");
        for skill in skills {
            write_skill(&work, skill, "v1").expect("write initial Skill");
        }
        run_git(&work, &["add", "-A"]);
        run_git(&work, &["commit", "-q", "-m", "initial"]);
        run_git(
            root.path(),
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().expect("work path"),
                remote.to_str().expect("remote path"),
            ],
        );
        Self {
            _root: root,
            work,
            remote,
            revision: AtomicUsize::new(1),
        }
    }

    pub(crate) fn source(&self) -> String {
        "https://git-fixture.invalid/skill-deck/remote.git".to_string()
    }

    fn local_source(&self) -> String {
        Url::from_file_path(&self.remote)
            .expect("bare repository file URL")
            .to_string()
    }

    pub(crate) fn commit_change(&self, skill: &str) {
        self.publish(skill, "change");
    }

    pub(crate) fn publish_change(&self, skill: &str) {
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.publish(skill, &format!("v{revision}"));
    }

    pub(crate) fn computed_hash(&self, skill: &str) -> String {
        let payload = build_skill_payload(&self.work.join(normalized_skill_path(skill)))
            .expect("build expected payload");
        compute_cli_project_hash_from_payload(&payload).expect("compute expected CLI hash")
    }

    fn publish(&self, skill: &str, version: &str) {
        write_skill(&self.work, skill, version).expect("write changed Skill");
        run_git(&self.work, &["add", "-A"]);
        run_git(&self.work, &["commit", "-q", "-m", version]);
        run_git(
            &self.work,
            &[
                "push",
                "-q",
                self.remote.to_str().expect("remote path"),
                "main",
            ],
        );
    }
}

fn normalized_skill_path(skill: &str) -> String {
    if skill.contains('/') {
        skill.to_string()
    } else {
        format!("skills/{skill}")
    }
}

fn write_skill(root: &Path, skill: &str, version: &str) -> Result<(), AppError> {
    let skill = normalized_skill_path(skill);
    let skill_root = root.join(&skill);
    let name = Path::new(&skill)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::InvalidSource {
            value: skill.clone(),
        })?;
    fs::create_dir_all(skill_root.join("scripts"))?;
    fs::create_dir_all(skill_root.join("references"))?;
    fs::create_dir_all(skill_root.join("assets"))?;
    fs::write(
        skill_root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {version} test skill\n---\n"),
    )?;
    fs::write(
        skill_root.join("scripts/run.sh"),
        format!("#!/bin/sh\necho {name}-{version}\n"),
    )?;
    fs::write(
        skill_root.join("references/guide.md"),
        format!("{name}-{version}-guide"),
    )?;
    fs::write(
        skill_root.join("assets/payload.bin"),
        format!("{name}-{version}-asset").as_bytes(),
    )?;
    Ok(())
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "Git fixture command failed: {args:?}; status={}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
