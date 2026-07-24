use std::ffi::OsStr;

#[cfg(target_os = "windows")]
const WINDOWS_CREATION_FLAGS: u32 = 0x0800_0000;

/// 构造不应显示终端窗口的同步子进程。
#[allow(
    clippy::disallowed_methods,
    reason = "这是 std::process::Command 的统一后台构造边界"
)]
pub(crate) fn std_command<S>(program: S) -> std::process::Command
where
    S: AsRef<OsStr>,
{
    let mut command = std::process::Command::new(program);
    apply_std_policy(&mut command);
    command
}

/// 构造不应显示终端窗口的异步子进程。
#[cfg(any(target_os = "windows", test))]
#[allow(
    clippy::disallowed_methods,
    reason = "这是 tokio::process::Command 的统一后台构造边界"
)]
pub(crate) fn tokio_command<S>(program: S) -> tokio::process::Command
where
    S: AsRef<OsStr>,
{
    let mut command = tokio::process::Command::new(program);
    apply_tokio_policy(&mut command);
    command
}

#[cfg(target_os = "windows")]
fn apply_std_policy(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(WINDOWS_CREATION_FLAGS);
}

#[cfg(not(target_os = "windows"))]
fn apply_std_policy(_command: &mut std::process::Command) {}

#[cfg(target_os = "windows")]
fn apply_tokio_policy(command: &mut tokio::process::Command) {
    command.creation_flags(WINDOWS_CREATION_FLAGS);
}

#[cfg(all(not(target_os = "windows"), test))]
fn apply_tokio_policy(_command: &mut tokio::process::Command) {}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    #[test]
    fn synchronous_constructor_preserves_the_requested_program() {
        let command = super::std_command("git");

        assert_eq!(command.get_program(), OsStr::new("git"));
    }

    #[tokio::test]
    async fn asynchronous_constructor_preserves_the_requested_program() {
        let command = super::tokio_command("wsl.exe");

        assert_eq!(command.as_std().get_program(), OsStr::new("wsl.exe"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_policy_uses_create_no_window() {
        assert_eq!(super::WINDOWS_CREATION_FLAGS, 0x0800_0000);
    }
}
