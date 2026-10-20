//! Explorer integration, all per user (`HKCU`, no administrator rights):
//!
//! * "Open with" availability for every format Folio reads (`OpenWithProgids`), so Folio shows up in the list;
//! * the app registration with its capabilities, so Folio appears in Settings → Default apps, where the user
//!   (and only the user: Windows protects the choice) can make it the default for a type;
//! * an optional "Read in Folio" entry in the right-click menu of those files.
//!
//! Every location is derived from `Env`, so tests run against a throw-away registry branch.

use crate::installer::{APP_NAME, Env};
use std::path::Path;

pub const PROG_ID: &str = "Folio.Document";
const MENU_KEY: &str = "Folio";

fn classes(env: &Env) -> String {
    format!(r"{}\Classes", env.software_key)
}

fn exts() -> Vec<&'static str> {
    folio_core::document_exts()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub open_with: bool,
    pub context_menu: bool,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{HKEY_CURRENT_USER, REG_NONE};

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn command(exe: &Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    fn set_default(key: &RegKey, v: &str) -> Result<(), String> {
        key.set_value("", &v.to_owned()).map_err(|e| e.to_string())
    }

    fn open_command(base: &str, exe: &Path, label: Option<&str>) -> Result<(), String> {
        let (k, _) = hkcu().create_subkey(base).map_err(|e| e.to_string())?;
        if let Some(l) = label {
            set_default(&k, l)?;
        }
        k.set_value("Icon", &format!("\"{}\",0", exe.display()))
            .map_err(|e| e.to_string())?;
        let (c, _) = hkcu()
            .create_subkey(format!(r"{base}\command"))
            .map_err(|e| e.to_string())?;
        set_default(&c, &command(exe))
    }

    pub fn register(env: &Env, exe: &Path, open_with: bool, context_menu: bool) -> Result<(), String> {
        let cls = classes(env);
        if open_with {
            let (k, _) = hkcu()
                .create_subkey(format!(r"{cls}\{PROG_ID}"))
                .map_err(|e| e.to_string())?;
            set_default(&k, crate::i18n::tr("Документ Folio", "Folio document"))?;
            let (icon, _) = hkcu()
                .create_subkey(format!(r"{cls}\{PROG_ID}\DefaultIcon"))
                .map_err(|e| e.to_string())?;
            set_default(&icon, &format!("\"{}\",0", exe.display()))?;
            open_command(&format!(r"{cls}\{PROG_ID}\shell\open"), exe, None)?;

            let app = format!(r"{cls}\Applications\{}", crate::installer::EXE_NAME);
            let (a, _) = hkcu().create_subkey(&app).map_err(|e| e.to_string())?;
            a.set_value("FriendlyAppName", &APP_NAME.to_owned())
                .map_err(|e| e.to_string())?;
            open_command(&format!(r"{app}\shell\open"), exe, None)?;

            let caps = format!(r"{}\{APP_NAME}\Capabilities", env.software_key);
            let (c, _) = hkcu().create_subkey(&caps).map_err(|e| e.to_string())?;
            c.set_value("ApplicationName", &APP_NAME.to_owned())
                .map_err(|e| e.to_string())?;
            c.set_value(
                "ApplicationDescription",
                &crate::i18n::tr(
                    "Читалка PDF, DOCX, EPUB, FB2 и других документов",
                    "Reader for PDF, DOCX, EPUB, FB2 and other documents",
                )
                .to_owned(),
            )
            .map_err(|e| e.to_string())?;
            let (assoc, _) = hkcu()
                .create_subkey(format!(r"{caps}\FileAssociations"))
                .map_err(|e| e.to_string())?;
            let (supported, _) = hkcu()
                .create_subkey(format!(r"{app}\SupportedTypes"))
                .map_err(|e| e.to_string())?;
            for ext in exts() {
                assoc
                    .set_value(format!(".{ext}"), &PROG_ID.to_owned())
                    .map_err(|e| e.to_string())?;
                supported
                    .set_value(format!(".{ext}"), &String::new())
                    .map_err(|e| e.to_string())?;
                let (ow, _) = hkcu()
                    .create_subkey(format!(r"{cls}\.{ext}\OpenWithProgids"))
                    .map_err(|e| e.to_string())?;
                ow.set_raw_value(
                    PROG_ID,
                    &RegValue {
                        bytes: Vec::new().into(),
                        vtype: REG_NONE,
                    },
                )
                .map_err(|e| e.to_string())?;
            }
            let (reg, _) = hkcu()
                .create_subkey(format!(r"{}\RegisteredApplications", env.software_key))
                .map_err(|e| e.to_string())?;
            reg.set_value(APP_NAME, &format!(r"{}\{APP_NAME}\Capabilities", env.software_key))
                .map_err(|e| e.to_string())?;
        }
        if context_menu {
            for ext in exts() {
                let base = format!(r"{cls}\SystemFileAssociations\.{ext}\shell\{MENU_KEY}");
                open_command(&base, exe, Some(crate::i18n::tr("Читать в Folio", "Read in Folio")))?;
            }
        }
        crate::winint::notify_associations_changed();
        Ok(())
    }

    /// Deletes `base\rel` and then its parents up to `base` for as long as they hold neither values nor subkeys.
    fn prune_empty(root: &RegKey, base: &str, rel: &str) {
        let mut parts: Vec<&str> = rel.split('\\').collect();
        while !parts.is_empty() {
            let path = format!(r"{base}\{}", parts.join(r"\"));
            if let Ok(k) = root.open_subkey(&path) {
                let empty = k.enum_keys().next().is_none() && k.enum_values().next().is_none();
                drop(k);
                if !empty {
                    break;
                }
                let _ = root.delete_subkey(&path);
            }
            parts.pop();
        }
    }

    pub fn unregister(env: &Env) {
        let cls = classes(env);
        let root = hkcu();
        let _ = root.delete_subkey_all(format!(r"{cls}\{PROG_ID}"));
        let _ = root.delete_subkey_all(format!(r"{cls}\Applications\{}", crate::installer::EXE_NAME));
        let _ = root.delete_subkey_all(format!(r"{}\{APP_NAME}", env.software_key));
        if let Ok(reg) = root.open_subkey_with_flags(
            format!(r"{}\RegisteredApplications", env.software_key),
            winreg::enums::KEY_SET_VALUE,
        ) {
            let _ = reg.delete_value(APP_NAME);
        }
        for ext in exts() {
            if let Ok(ow) =
                root.open_subkey_with_flags(format!(r"{cls}\.{ext}\OpenWithProgids"), winreg::enums::KEY_SET_VALUE)
            {
                let _ = ow.delete_value(PROG_ID);
            }
            let _ = root.delete_subkey_all(format!(r"{cls}\SystemFileAssociations\.{ext}\shell\{MENU_KEY}"));
            // keys that only existed for Folio go too; anything somebody else put there stays
            prune_empty(&root, &cls, &format!(r".{ext}\OpenWithProgids"));
            prune_empty(&root, &cls, &format!(r"SystemFileAssociations\.{ext}\shell"));
        }
        prune_empty(&root, &cls, "Applications");
        prune_empty(&root, &env.software_key, "RegisteredApplications");
        crate::winint::notify_associations_changed();
    }

    pub fn state(env: &Env) -> State {
        let cls = classes(env);
        let root = hkcu();
        State {
            open_with: root.open_subkey(format!(r"{cls}\{PROG_ID}")).is_ok(),
            context_menu: root
                .open_subkey(format!(r"{cls}\SystemFileAssociations\.pdf\shell\{MENU_KEY}"))
                .is_ok(),
        }
    }

    /// Removes the whole sandbox branch of a test.
    #[cfg(test)]
    pub fn drop_branch(env: &Env) {
        let _ = hkcu().delete_subkey_all(&env.software_key);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn register(_: &Env, _: &Path, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }
    pub fn unregister(_: &Env) {}
    pub fn state(_: &Env) -> State {
        State {
            open_with: false,
            context_menu: false,
        }
    }
    #[cfg(test)]
    pub fn drop_branch(_: &Env) {}
}

/// Registers Folio for the formats it reads; each part is optional.
pub fn register(env: &Env, exe: &Path, open_with: bool, context_menu: bool) -> Result<(), String> {
    imp::register(env, exe, open_with, context_menu)
}

/// Removes every registration made by `register`.
pub fn unregister(env: &Env) {
    imp::unregister(env)
}

pub fn state(env: &Env) -> State {
    imp::state(env)
}

/// What a fresh install turns on: availability in "Open with" and Default apps (the menu entry is the user's choice).
pub fn register_if_enabled(env: &Env, exe: &Path) {
    let _ = register(env, exe, true, false);
}

/// Opens Settings → Default apps on Folio's page.
pub fn open_default_apps_settings() {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:defaultapps"])
        .spawn();
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    fn sandbox(tag: &str) -> Env {
        let dir = std::env::temp_dir().join("folio-shell-test");
        Env {
            install_dir: dir.join("Programs"),
            start_menu_dir: dir.join("StartMenu"),
            software_key: format!(r"Software\FolioShellTest-{tag}-{}", std::process::id()),
        }
    }

    #[test]
    fn registration_round_trip_in_a_sandbox_branch() {
        let env = sandbox("a");
        let exe = Path::new(r"C:\Fake\Folio.exe");
        register(&env, exe, true, true).unwrap();
        let st = state(&env);
        assert!(st.open_with && st.context_menu);

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let cmd: String = hkcu
            .open_subkey(format!(r"{}\Classes\{PROG_ID}\shell\open\command", env.software_key))
            .unwrap()
            .get_value("")
            .unwrap();
        assert_eq!(cmd, r#""C:\Fake\Folio.exe" "%1""#);
        assert!(
            hkcu.open_subkey(format!(r"{}\Classes\.pdf\OpenWithProgids", env.software_key))
                .unwrap()
                .enum_values()
                .any(|v| v.is_ok_and(|(n, _)| n == PROG_ID))
        );
        let caps: String = hkcu
            .open_subkey(format!(r"{}\RegisteredApplications", env.software_key))
            .unwrap()
            .get_value(APP_NAME)
            .unwrap();
        assert!(caps.ends_with(r"Folio\Capabilities"));

        unregister(&env);
        let st = state(&env);
        assert!(!st.open_with && !st.context_menu);
        let classes = hkcu.open_subkey(format!(r"{}\Classes", env.software_key)).unwrap();
        assert_eq!(classes.enum_keys().count(), 0, "no empty leftovers under Classes");
        imp::drop_branch(&env);
    }

    #[test]
    fn menu_entry_is_independent_of_open_with() {
        let env = sandbox("b");
        register(&env, Path::new(r"C:\Fake\Folio.exe"), true, false).unwrap();
        assert_eq!(
            state(&env),
            State {
                open_with: true,
                context_menu: false
            }
        );
        unregister(&env);
        imp::drop_branch(&env);
    }
}
