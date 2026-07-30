use std::io::ErrorKind;

use anyhow::Context;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "Lyra";

fn startup_command() -> anyhow::Result<String> {
    let exe = std::env::current_exe().context("could not determine current executable path")?;
    Ok(format!("\"{}\"", exe.display()))
}

pub fn is_enabled() -> anyhow::Result<bool> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey(RUN_KEY_PATH) {
        Ok(key) => key,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).context("could not open Run registry key"),
    };

    match key.get_value::<String, _>(RUN_VALUE_NAME) {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).context("could not read Lyra startup registry value"),
    }
}

pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(RUN_KEY_PATH)
        .context("could not open or create Run registry key")?;

    if enabled {
        key.set_value(RUN_VALUE_NAME, &startup_command()?)
            .context("could not write Lyra startup registry value")?;
    } else {
        match key.delete_value(RUN_VALUE_NAME) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("could not delete Lyra startup registry value"),
        }
    }

    Ok(())
}

pub fn toggle() -> anyhow::Result<bool> {
    let next = !is_enabled()?;
    set_enabled(next)?;
    Ok(next)
}
