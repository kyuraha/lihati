use std::path::Path;

const PROGID: &str = "Lihati.Markdown";
const EXE_DESC: &str = "Lihati Markdown Document";

fn command_value(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::io;
    use winreg::enums::*;
    use winreg::RegKey;

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    pub fn register(exe: &Path) -> io::Result<()> {
        let hkcu = hkcu();
        let cmd = command_value(exe);

        let (app, _) = hkcu.create_subkey("Software\\Classes\\Applications\\lihati.exe")?;
        app.set_value("", &EXE_DESC)?;
        let (sh, _) = hkcu.create_subkey("Software\\Classes\\Applications\\lihati.exe\\shell\\open\\command")?;
        sh.set_value("", &cmd)?;

        let (prog, _) = hkcu.create_subkey(format!("Software\\Classes\\{PROGID}"))?;
        prog.set_value("", &EXE_DESC)?;
        prog.set_value("FriendlyTypeName", &EXE_DESC)?;
        let (open, _) = hkcu.create_subkey(format!("Software\\Classes\\{PROGID}\\shell\\open"))?;
        open.set_value("FriendlyAppName", &"Lihati")?;
        let (cmdk, _) = hkcu.create_subkey(format!("Software\\Classes\\{PROGID}\\shell\\open\\command"))?;
        cmdk.set_value("", &cmd)?;

        let (ext, _) = hkcu.create_subkey("Software\\Classes\\.md\\OpenWithProgids")?;
        ext.set_value(PROGID, &"")?;

        Ok(())
    }

    pub fn unregister() -> io::Result<()> {
        let hkcu = hkcu();
        let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\{PROGID}"));
        let _ = hkcu.delete_subkey_all("Software\\Classes\\Applications\\lihati.exe");
        if let Ok(ext) = hkcu.open_subkey_with_flags("Software\\Classes\\.md\\OpenWithProgids", KEY_SET_VALUE) {
            let _ = ext.delete_value(PROGID);
        }
        Ok(())
    }
}

pub fn register() -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map_err(|e| format!("cannot resolve exe path: {e}"))?;
        imp::register(&exe).map_err(|e| format!("registry error: {e}"))?;
        println!("Registered Lihati in the \"Open with\" menu for .md files.");
        println!("Right-click any Markdown file > Open with > Lihati.");
        println!("Exe: {}", exe.display());
        Ok(())
    }
    #[cfg(not(windows))]
    Err("only supported on Windows".into())
}

pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    {
        imp::unregister().map_err(|e| format!("registry error: {e}"))?;
        println!("Unregistered Lihati file association.");
        Ok(())
    }
    #[cfg(not(windows))]
    Err("only supported on Windows".into())
}
