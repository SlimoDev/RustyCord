#[cfg(feature="offline-probes")]
fn main(){
    let value=match notdiscord::controller::scan_game_processes_once(){Ok(value)=>value,Err(error)=>serde_json::json!({"error":error})};
    println!("{value}");
}
#[cfg(not(feature="offline-probes"))]
fn main(){eprintln!("Este sondeo requiere la opción offline-probes.");}
