use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// Must be constructed on the same thread that runs the UI event loop, and
/// kept alive for the app's lifetime: `GlobalHotKeyManager` creates a hidden
/// window that only receives `WM_HOTKEY` messages while that thread's message
/// loop is being pumped, and dropping it destroys that window (unregistering
/// the hotkey).
pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
    toggle_id: u32,
}

impl HotkeyManager {
    pub fn new() -> anyhow::Result<Self> {
        let manager = GlobalHotKeyManager::new()?;
        let toggle = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyL);
        manager.register(toggle)?;

        Ok(Self {
            _manager: manager,
            toggle_id: toggle.id,
        })
    }

    /// Non-blocking. Returns `true` if the toggle hotkey was pressed since the
    /// last call.
    pub fn poll_toggle(&self) -> bool {
        let mut toggled = false;
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.toggle_id && event.state == HotKeyState::Pressed {
                toggled = true;
            }
        }
        toggled
    }
}
