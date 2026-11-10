use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::inject::Injector;
use crate::platform::{self, WindowInfo};
use crate::protocol::ServerMsg;

const FOCUS_TIMEOUT: Duration = Duration::from_millis(800);
const FOCUS_POLL: Duration = Duration::from_millis(20);
/// 前面化の直後は対象アプリがまだ入力を受け付けないことがあるため少し待つ
const FOCUS_SETTLE: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    NoTarget,
    /// 対象ウィンドウが見つからない（閉じられた・作り直された）。
    /// 呼び出し側はこれを見たら同じアプリ名で探し直すとよい。
    WindowGone,
    FocusFailed,
    Inject(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::NoTarget => write!(f, "対象ウィンドウが未選択です"),
            Failure::WindowGone => write!(f, "対象ウィンドウが見つかりません"),
            Failure::FocusFailed => write!(f, "対象を前面にできません"),
            Failure::Inject(e) => write!(f, "{e}"),
        }
    }
}

/// 1操作を自機の対象ウィンドウへ適用する。クリックもキーも必ずここを通る。
/// 戻り値は「前面化が必要だったか」。
pub fn apply(
    injector: &mut Option<Injector>,
    target: Option<&WindowInfo>,
    msg: &ServerMsg,
) -> Result<bool, Failure> {
    let target = target.ok_or(Failure::NoTarget)?;
    // ここでウィンドウの実在を確かめる。閉じられた・作り直された場合はここで検出する
    let rect = platform::window_rect(target.id).ok_or(Failure::WindowGone)?;

    if injector.is_none() {
        *injector = Some(Injector::new().map_err(Failure::Inject)?);
    }
    let injector = injector.as_mut().unwrap();

    // 前面にないまま注入すると別のウィンドウへ入るので、確認できなければ注入しない
    let needs_focus = !platform::is_frontmost(target.id, target.pid);
    if needs_focus {
        platform::focus_window(target.id, target.pid);
        let deadline = Instant::now() + FOCUS_TIMEOUT;
        while !platform::is_frontmost(target.id, target.pid) {
            if Instant::now() >= deadline {
                return Err(Failure::FocusFailed);
            }
            sleep(FOCUS_POLL);
        }
        sleep(FOCUS_SETTLE);
    }

    match msg {
        ServerMsg::Click { x, y, button, .. } => {
            injector.click_in_rect(rect, *x, *y, *button).map_err(Failure::Inject)?;
        }
        ServerMsg::Key { code, .. } => injector.tap(code).map_err(Failure::Inject)?,
        ServerMsg::Reset | ServerMsg::Ping => {}
    }
    Ok(needs_focus)
}
