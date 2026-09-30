# Delivery checklist

Inspect the build output, compare the supported environments, and keep the evidence with the change. Run `pnpm test` before packaging the application.

## Build commands

```sh
pnpm exec playwright test --project=desktop-review --reporter=list --output=artifacts/desktop-review/current --grep="navigation, reading position, clipboard, and keyboard focus"
```

```powershell
$output = 'C:\Review\Skill Deck\artifacts'
Write-Output "Review output: $output"
```

## Checks

| Check | Result |
| --- | --- |
| Content | Preserved |
| Copy | Exact text |

## Environment comparison

| Environment | Renderer | Build profile | Source type | Read behavior | Copy behavior | Keyboard focus | Review evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Windows | WebView2 | Debug review | GitHub | Keep the current document | Preserve spaces and line breaks | Return to the initiating control | Native screenshots and system clipboard |
| Linux | WebKitGTK | Debug review | Local directory | Keep the current document | Preserve spaces and line breaks | Return to the initiating control | Native screenshots and input checks |
| macOS | WKWebView | Pending verification | Well-known | Verify on the target platform | Verify on the target platform | Verify on the target platform | Record the actual coverage |

## Review notes

> Check the result in the target runtime before marking the task complete.
>
> 核对实际构建的界面、复制结果与键盘焦点，并分别记录各平台的验证范围。

- Read the current contract.
  - Follow the public entry point.
  - Inspect the failure path.
- Verify the visible result.

1. Prepare a focused change.

   Keep the supporting explanation with the affected behavior.

2. Record the outcome.

   Preserve useful context for the next review.

The generated file name may be long: `desktop-validation-report-for-windows-webview2-and-linux-webkitgtk-current-revision.json`.

## Structured output

```json
{
  "status": "ready",
  "checks": ["content", "clipboard", "keyboard"],
  "note": "Keep <tags> & spaces unchanged."
}
```
