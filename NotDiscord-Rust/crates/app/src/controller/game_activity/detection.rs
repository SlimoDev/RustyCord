//! Pure, bounded detection. Executable paths and unparsed window titles never leave the scanner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Program { Game(&'static str, bool), Rpcs3 }
// New identities are documented by their publishers, not guessed from arbitrary window titles:
// larian.com/support/faqs/the-larian-launcher-is-crashing_63 (BG3, not LariLauncher)
// support.cdprojektred.com/en/cyberpunk/pc/sp-technical/issue/1568/contact-to-support
// support.rockstargames.com/articles/2qjn6j48MfhpmlfEuaLAZt/red-dead-redemption-2-on-pc-exited-unexpectedly
// help.bethesda.net/app/answers/detail/a_id/45370 (game executables, not launchers)
// support.warframe.com/hc/en-us/articles/201541334-Basic-Network-Troubleshooting-Guide
// wiki.factorio.com/Command_line_parameters (also supports headless mode, so require a window)
const CATALOG:&[(&str,&str,bool)]=&[
    ("totclient-win64-shipping.exe","The Outlast Trials",false),
    ("totclient.exe","The Outlast Trials",false),
    ("outlasttrials.exe","The Outlast Trials",false),
    ("fortniteclient-win64-shipping.exe","Fortnite",false),
    ("valorant-win64-shipping.exe","VALORANT",false),
    ("cs2.exe","Counter-Strike 2",false),
    ("league of legends.exe","League of Legends",false),
    ("overwatch.exe","Overwatch",false),
    ("bg3.exe","Baldur's Gate 3",false),
    ("bg3_dx11.exe","Baldur's Gate 3",false),
    ("cyberpunk2077.exe","Cyberpunk 2077",false),
    ("rdr2.exe","Red Dead Redemption 2",false),
    ("skyrimse.exe","The Elder Scrolls V: Skyrim Special Edition",false),
    ("fallout4.exe","Fallout 4",false),
    ("warframe.x64.exe","Warframe",false),
    ("factorio.exe","Factorio",true),
];
pub(super) fn identify(executable:&str)->Option<Program> {
    if executable.len()>16*1024||executable.chars().any(char::is_control){return None;}
    let name=executable.rsplit(['\\','/']).next()?;
    if name.eq_ignore_ascii_case("rpcs3.exe"){return Some(Program::Rpcs3);}
    CATALOG.iter().find(|(exe,_,_)|name.eq_ignore_ascii_case(exe)).map(|(_,name,window)|Program::Game(name,*window))
}

#[derive(Clone,Debug)]
pub(super) struct Process {pub pid:u32,pub program:Program}
#[derive(Clone,Debug)]
pub(super) struct Window {pub pid:u32,pub foreground:bool,pub rpcs3_title:Option<String>}
#[derive(Clone,Debug,Default,PartialEq)]
pub(super) struct ScanResult {pub activity:Option<String>,pub rpcs3_open:bool}

/// RPCS3's default game window is "%F | %R | %V | %T [%t]", with an "FPS: " prefix.
/// Its main window is "RPCS3 " + version and must never be mistaken for a running game.
/// Sources: RPCS3/rpcs3 master rpcs3/Emu/system_config.h (Window Title Format),
/// rpcs3/Emu/System.cpp (GetFormattedTitle), rpcs3/rpcs3qt/gs_frame.cpp and main_window.cpp.
pub(super) fn rpcs3_game(title:&str)->Option<String> {
    if title.len()>4096||title.chars().any(char::is_control){return None;}
    let parts=title.split(" | ").collect::<Vec<_>>();
    if parts.len()!=4||!parts[0].starts_with("FPS: ")||!matches!(parts[1],"Vulkan"|"OpenGL") {return None;}
    let (name,id)=parts[3].rsplit_once(" [")?;
    let id=id.strip_suffix(']')?;
    if id.len()!=9||!id.as_bytes()[..4].iter().all(u8::is_ascii_uppercase)
        ||!id.as_bytes()[4..].iter().all(u8::is_ascii_digit){return None;}
    let name=name.trim();let lower=name.to_ascii_lowercase();
    if name.is_empty()||name.chars().count()>112||name.contains('\\')||name.contains(":/")
        ||name.starts_with('/')||lower.contains("www.")||lower.starts_with("file:") {return None;}
    Some(format!("{name} (RPCS3)"))
}

pub(super) fn choose(processes:&[Process],windows:&[Window],previous:Option<&str>)->ScanResult {
    let mut candidates=Vec::<(String,bool)>::new();
    let mut rpcs3_open=false;
    for process in processes.iter().take(64) {
        let matching=windows.iter().filter(|window|window.pid==process.pid).collect::<Vec<_>>();
        match process.program {
            Program::Game(name,requires_window) => {
                if !requires_window||!matching.is_empty(){candidates.push((name.into(),matching.iter().any(|window|window.foreground)));}
            },
            Program::Rpcs3 => {
                rpcs3_open=true;
                for window in matching {
                    if let Some(title)=window.rpcs3_title.as_deref().and_then(rpcs3_game) {candidates.push((title,window.foreground));}
                }
            },
        }
    }
    // Prefer the game being used. When focus goes to the chat or another app, keep the
    // still-running choice instead of bouncing between alphabetically sorted games.
    candidates.sort_by(|a,b|b.1.cmp(&a.1).then_with(||(Some(b.0.as_str())==previous).cmp(&(Some(a.0.as_str())==previous))).then(a.0.cmp(&b.0)));
    let activity=candidates.into_iter().next().map(|entry|entry.0).or_else(||rpcs3_open.then(||"RPCS3".into()));
    ScanResult{activity,rpcs3_open}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_executables_expand_catalog_but_never_match_launchers_or_arguments() {
        for (exe,game,requires_window) in CATALOG {
            assert_eq!(identify(&exe.to_uppercase()),Some(Program::Game(game,*requires_window)));
            assert_eq!(identify(&format!(r"X:\Games\{exe}")),Some(Program::Game(game,*requires_window)));
            assert!(identify(&format!("{exe}.backup")).is_none());
        }
        for exe in ["LariLauncher.exe","SkyrimSELauncher.exe","Fallout4Launcher.exe","launcher.exe","steam.exe","javaw.exe","rpcs3-updater.exe","rpcs3.exe --game=secret"] {assert!(identify(exe).is_none());}
        assert_eq!(identify("RPCS3.EXE"),Some(Program::Rpcs3));
    }
    #[test]
    fn rpcs3_only_publishes_valid_default_game_window_not_manager_or_private_data() {
        assert_eq!(rpcs3_game("FPS: 59.90 | Vulkan | Example adapter | Persona 5 [BLUS31604]").as_deref(),Some("Persona 5 (RPCS3)"));
        assert_eq!(rpcs3_game("FPS: 0.00 | OpenGL | Example adapter | Demon's Souls [BLES00932]").as_deref(),Some("Demon's Souls (RPCS3)"));
        for title in ["RPCS3 v0.0.38-12345 Alpha","Settings","Persona 5 [BLUS31604]","FPS: 60 | Vulkan | Adapter | Loading…",r"FPS: 60 | Vulkan | Adapter | C:\Users\Person\secret [BLUS31604]","FPS: 60 | Vulkan | Adapter | https://private.example [BLUS31604]","FPS: 60 | Vulkan | Adapter | Game [bad000001]","FPS: 60 | Vulkan | Adapter | Game [BLUS31604] (stopping)"] {assert!(rpcs3_game(title).is_none(),"{title}");}
    }
    #[test]
    fn foreground_wins_then_previous_stays_stable_and_exiting_clears_activity() {
        let processes=[Process{pid:10,program:Program::Game("Overwatch",false)},Process{pid:20,program:Program::Game("Baldur's Gate 3",false)}];
        let windows=[Window{pid:10,foreground:true,rpcs3_title:None}];
        assert_eq!(choose(&processes,&windows,Some("Baldur's Gate 3")).activity.as_deref(),Some("Overwatch"));
        assert_eq!(choose(&processes,&[],Some("Overwatch")).activity.as_deref(),Some("Overwatch"));
        assert_eq!(choose(&processes[..1],&[],Some("Baldur's Gate 3")).activity.as_deref(),Some("Overwatch"));
        assert!(choose(&[],&windows,Some("Overwatch")).activity.is_none());
    }
    #[test]
    fn emulator_idle_and_headless_factorio_are_not_fake_games() {
        let processes=[Process{pid:10,program:Program::Rpcs3},Process{pid:20,program:Program::Game("Factorio",true)}];
        let idle=choose(&processes,&[],None);assert!(idle.rpcs3_open);assert_eq!(idle.activity.as_deref(),Some("RPCS3"));
        let windows=[Window{pid:10,foreground:true,rpcs3_title:Some("RPCS3 v0.0.38".into())},Window{pid:20,foreground:false,rpcs3_title:None}];
        assert_eq!(choose(&processes,&windows,None).activity.as_deref(),Some("Factorio"));
        let playing=[Window{pid:10,foreground:true,rpcs3_title:Some("FPS: 60 | Vulkan | Adapter | Persona 5 [BLUS31604]".into())}];
        assert_eq!(choose(&processes,&playing,None).activity.as_deref(),Some("Persona 5 (RPCS3)"));
        assert!(choose(&[Process{pid:99,program:Program::Game("Overwatch",false)}],&playing,None).activity.as_deref()==Some("Overwatch"));
    }
}
