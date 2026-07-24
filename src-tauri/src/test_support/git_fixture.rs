#![allow(
    clippy::disallowed_methods,
    reason = "该 test-support 模块需要直接调用真实 Git 构建测试仓库"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tempfile::TempDir;
use url::Url;

use crate::application::git_transport::{GitSourceTransport, ProcessGitTransport};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{build_skill_payload, compute_cli_project_hash_from_payload};
use crate::core::{ClonePhase, CloneProgress, CloneResult};
use crate::error::AppError;

/// 不启动 Git process 的可变 Skill tree，用于 application 层的确定性测试。
pub(crate) struct SkillTreeFixture {
    _root: TempDir,
    work: PathBuf,
    revision: std::sync::Arc<AtomicUsize>,
}

impl SkillTreeFixture {
    pub(crate) fn new(skills: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("Skill tree fixture tempdir");
        let work = root.path().join("work");
        fs::create_dir_all(&work).expect("create Skill tree fixture");
        for skill in skills {
            write_skill(&work, skill, "v1").expect("write initial Skill");
        }
        Self {
            _root: root,
            work,
            revision: std::sync::Arc::new(AtomicUsize::new(1)),
        }
    }

    pub(crate) fn source(&self) -> String {
        "https://git-fixture.invalid/skill-deck/remote.git".to_string()
    }

    pub(crate) fn commit_change(&self, skill: &str) {
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        write_skill(&self.work, skill, &format!("v{revision}")).expect("write changed Skill");
    }
}

#[derive(Debug)]
pub(crate) struct DeterministicGitTransport {
    source: String,
    source_root: PathBuf,
    revision: std::sync::Arc<AtomicUsize>,
    clone_count: AtomicUsize,
    reject_probes: AtomicBool,
}

impl DeterministicGitTransport {
    pub(crate) fn for_fixture(fixture: &SkillTreeFixture) -> Self {
        Self {
            source: fixture.source(),
            source_root: fixture.work.clone(),
            revision: fixture.revision.clone(),
            clone_count: AtomicUsize::new(0),
            reject_probes: AtomicBool::new(false),
        }
    }

    pub(crate) fn clone_count(&self) -> usize {
        self.clone_count.load(Ordering::SeqCst)
    }

    pub(crate) fn reject_probes(&self) {
        self.reject_probes.store(true, Ordering::SeqCst);
    }

    fn revision(&self) -> String {
        format!("fixture-revision-{}", self.revision.load(Ordering::SeqCst))
    }

    fn validate_source(&self, source: &str) -> Result<(), AppError> {
        if source == self.source {
            Ok(())
        } else {
            Err(AppError::InvalidSource {
                value: source.to_string(),
            })
        }
    }
}

impl GitSourceTransport for DeterministicGitTransport {
    fn clone_source(
        &self,
        url: &str,
        _git_ref: Option<&str>,
        on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError> {
        self.validate_source(url)?;
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        self.clone_count.fetch_add(1, Ordering::SeqCst);
        on_progress(CloneProgress {
            phase: ClonePhase::Connecting,
            elapsed_secs: 0,
            timeout_secs: 0,
            message: None,
        });
        let temp_dir = tempfile::tempdir()?;
        copy_tree(&self.source_root, temp_dir.path())?;
        on_progress(CloneProgress {
            phase: ClonePhase::Done,
            elapsed_secs: 0,
            timeout_secs: 0,
            message: None,
        });
        Ok(CloneResult {
            repo_path: temp_dir.path().to_path_buf(),
            ref_revision: Some(self.revision()),
            _temp_dir: temp_dir,
        })
    }

    fn probe_ref_revision(
        &self,
        url: &str,
        _git_ref: Option<&str>,
        cancellation: CancellationSignal,
    ) -> Result<String, AppError> {
        self.validate_source(url)?;
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        if self.reject_probes.load(Ordering::SeqCst) {
            return Err(AppError::GitCloneFailed {
                message: "injected remote ref probe failure".to_string(),
            });
        }
        Ok(self.revision())
    }
}

/// 对真实 process Git 做计数，并将公开 fixture URL 映射到本地 `file://` remote。
#[derive(Debug, Default)]
pub(crate) struct CountingGitTransport {
    process: ProcessGitTransport,
    clone_count: AtomicUsize,
    public_source: Option<String>,
    local_source: Option<String>,
}

impl CountingGitTransport {
    pub(crate) fn for_repo(repo: &BareSkillRepo) -> Self {
        Self {
            process: ProcessGitTransport,
            clone_count: AtomicUsize::new(0),
            public_source: Some(repo.source()),
            local_source: Some(repo.local_source()),
        }
    }

    pub(crate) fn clone_count(&self) -> usize {
        self.clone_count.load(Ordering::SeqCst)
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

    pub(crate) fn local_source(&self) -> String {
        Url::from_file_path(&self.remote)
            .expect("bare repository file URL")
            .to_string()
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

fn copy_tree(source: &Path, destination: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
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
