use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::inject::Injector;
use crate::platform::{self, WindowInfo};
use crate::protocol::ServerMsg;

const FOCUS_TIMEOUT: Duration = Duration::from_millis(800);
const FOCUS_POLL: Duration = Duration::from_millis(20);
/// 前面化の直後は対象アプリがまだ入力を受け付けないことがあるため少し待つ
const FOCUS_SETTLE: Duration = Duration::from_millis(60);

/// 1操作を自機の対象ウィンドウへ適用する。クリックもキーも必ずここを通る。
/// 戻り値は「前面化が必要だったか」。
pub fn apply(
    injector: &mut Option<Injector>,
    target: Option<&WindowInfo>,
    msg: &ServerMsg,
) -> Result<bool, String> {
    let target = target.ok_or("対象ウィンドウが未選択です")?;
    if injector.is_none() {
        *injector = Some(Injector::new()?);
    }
    let injector = injector.as_mut().unwrap();

    // 前面にないまま注入すると別のウィンドウへ入るので、確認できなければ注入しない
    let focused = !platform::is_frontmost(target.id, target.pid);
    if focused {
        platform::focus_window(target.id, target.pid);
        let deadline = Instant::now() + FOCUS_TIMEOUT;
        while !platform::is_frontmost(target.id, target.pid) {
            if Instant::now() >= deadline {
                return Err(format!("{} を前面にできません", target.app_name));
            }
            sleep(FOCUS_POLL);
        }
        sleep(FOCUS_SETTLE);
    }

    match msg {
        ServerMsg::Click { x, y, button, .. } => {
            let rect = platform::window_rect(target.id)
                .ok_or_else(|| format!("{} のウィンドウが見つかりません", target.app_name))?;
            injector.click_in_rect(rect, *x, *y, *button)?;
        }
        ServerMsg::Key { code, .. } => injector.tap(code)?,
    }
    Ok(focused)
}
