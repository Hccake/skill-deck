use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::skill_libraries::{
    merge_unknown_source_fields, validate_catalog, LibraryCatalog, LibraryId, LibrarySkillRecord,
    RetiredLibrarySkillRecord, RetirementId,
};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub(crate) enum MembershipChange {
    Upsert(LibrarySkillRecord),
    Retire {
        retirement_id: RetirementId,
        retired_at: String,
    },
    Purge {
        retirement_id: RetirementId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MembershipChangeKind {
    Added,
    Updated,
    Retired,
    Reactivated,
    Purged,
}

pub(crate) fn apply_membership_change(
    catalog: &mut LibraryCatalog,
    library_id: &LibraryId,
    member_name: &str,
    change: MembershipChange,
) -> Result<MembershipChangeKind, AppError> {
    validate_catalog(catalog)?;
    let library = catalog
        .libraries
        .iter_mut()
        .find(|library| &library.id == library_id)
        .ok_or_else(|| AppError::PathNotFound {
            path: library_id.as_str().to_string(),
        })?;
    let result = match change {
        MembershipChange::Upsert(mut replacement) => {
            if replacement.name != member_name {
                return Err(AppError::StaleTarget);
            }
            let directory = InstalledSkillResolver::install_dir_name(member_name)?;
            for active in &library.skills {
                if active.name != member_name
                    && InstalledSkillResolver::install_dir_name(&active.name)? == directory
                {
                    return Err(name_conflict());
                }
            }
            for retired in &library.retired_skills {
                if retired.member.name != member_name
                    && InstalledSkillResolver::install_dir_name(&retired.member.name)? == directory
                {
                    return Err(name_conflict());
                }
            }
            if let Some(current) = library
                .skills
                .iter_mut()
                .find(|member| member.name == member_name)
            {
                preserve_unknown_fields(&mut replacement, current);
                *current = replacement;
                MembershipChangeKind::Updated
            } else if let Some(index) = library
                .retired_skills
                .iter()
                .position(|retired| retired.member.name == member_name)
            {
                let retired = library.retired_skills.remove(index);
                preserve_unknown_fields(&mut replacement, &retired.member);
                library.skills.push(replacement);
                library
                    .skills
                    .sort_by(|left, right| left.name.cmp(&right.name));
                MembershipChangeKind::Reactivated
            } else {
                library.skills.push(replacement);
                library
                    .skills
                    .sort_by(|left, right| left.name.cmp(&right.name));
                MembershipChangeKind::Added
            }
        }
        MembershipChange::Retire {
            retirement_id,
            retired_at,
        } => {
            if retirement_id.as_str().is_empty() || retired_at.is_empty() {
                return Err(AppError::StaleTarget);
            }
            let index = library
                .skills
                .iter()
                .position(|member| member.name == member_name)
                .ok_or_else(|| AppError::PathNotFound {
                    path: member_name.to_string(),
                })?;
            let member = library.skills.remove(index);
            library.retired_skills.push(RetiredLibrarySkillRecord {
                retirement_id,
                member,
                retired_at,
                extra: serde_json::Map::new(),
            });
            library
                .retired_skills
                .sort_by(|left, right| left.member.name.cmp(&right.member.name));
            MembershipChangeKind::Retired
        }
        MembershipChange::Purge { retirement_id } => {
            let index = library
                .retired_skills
                .iter()
                .position(|retired| retired.member.name == member_name)
                .ok_or_else(|| AppError::PathNotFound {
                    path: member_name.to_string(),
                })?;
            if library.retired_skills[index].retirement_id != retirement_id {
                return Err(AppError::StaleTarget);
            }
            library.retired_skills.remove(index);
            MembershipChangeKind::Purged
        }
    };
    validate_catalog(catalog)?;
    Ok(result)
}

fn preserve_unknown_fields(replacement: &mut LibrarySkillRecord, current: &LibrarySkillRecord) {
    replacement.extra = current.extra.clone();
    merge_unknown_source_fields(&mut replacement.source_record, &current.source_record);
}

fn name_conflict() -> AppError {
    AppError::Validation {
        field: Some("skillName".to_string()),
        message: "Skill name conflicts with an active or retired Library member".to_string(),
    }
}
