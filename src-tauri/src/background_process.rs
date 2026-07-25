use std::ffi::OsStr;

#[cfg(target_os = "windows")]
const WINDOWS_CREATE_NO_WINDOW_FLAG: u32 = 0x0800_0000;

#[cfg(test)]
std::thread_local! {
    static STD_POLICY_APPLICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TOKIO_POLICY_APPLICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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

    #[cfg(test)]
    record_std_policy_application();
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW_FLAG);
}

#[cfg(not(target_os = "windows"))]
fn apply_std_policy(_command: &mut std::process::Command) {
    #[cfg(test)]
    record_std_policy_application();
}

#[cfg(target_os = "windows")]
fn apply_tokio_policy(command: &mut tokio::process::Command) {
    #[cfg(test)]
    record_tokio_policy_application();
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW_FLAG);
}

#[cfg(all(not(target_os = "windows"), test))]
fn apply_tokio_policy(_command: &mut tokio::process::Command) {
    record_tokio_policy_application();
}

#[cfg(test)]
fn record_std_policy_application() {
    STD_POLICY_APPLICATIONS.set(STD_POLICY_APPLICATIONS.get() + 1);
}

#[cfg(test)]
fn std_policy_application_count() -> usize {
    STD_POLICY_APPLICATIONS.get()
}

#[cfg(test)]
fn record_tokio_policy_application() {
    TOKIO_POLICY_APPLICATIONS.set(TOKIO_POLICY_APPLICATIONS.get() + 1);
}

#[cfg(test)]
fn tokio_policy_application_count() -> usize {
    TOKIO_POLICY_APPLICATIONS.get()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    #[test]
    fn synchronous_constructor_preserves_the_requested_program() {
        let policy_applications = super::std_policy_application_count();
        let command = super::std_command("git");

        assert_eq!(command.get_program(), OsStr::new("git"));
        assert_eq!(
            super::std_policy_application_count(),
            policy_applications + 1
        );
    }

    #[tokio::test]
    async fn asynchronous_constructor_preserves_the_requested_program() {
        let policy_applications = super::tokio_policy_application_count();
        let command = super::tokio_command("wsl.exe");

        assert_eq!(command.as_std().get_program(), OsStr::new("wsl.exe"));
        assert_eq!(
            super::tokio_policy_application_count(),
            policy_applications + 1
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_policy_uses_create_no_window() {
        assert_eq!(super::WINDOWS_CREATE_NO_WINDOW_FLAG, 0x0800_0000);
    }
}
