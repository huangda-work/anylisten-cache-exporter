use anylisten_core::library::scan;
use anylisten_core::platform::discover_data_root;
use anylisten_core::store::read_cached_audio;

fn main() -> Result<(), String> {
    let root = discover_data_root(None)?;
    let result = scan(&root)?;
    let named = result
        .cached
        .iter()
        .filter(|track| !track.title.starts_with("缓存 "))
        .count();
    println!(
        "cached={} remote={} unavailable={} named={}",
        result.cached.len(),
        result.remote.len(),
        result.unavailable_count,
        named
    );
    if std::env::args().any(|argument| argument == "--verify") {
        let mut bytes = 0;
        for track in &result.cached {
            let audio = read_cached_audio(track)?;
            if audio.len() != track.total_size {
                return Err("缓存读取长度不匹配".into());
            }
            bytes += audio.len();
        }
        println!("verified={} bytes={bytes}", result.cached.len());
    }
    Ok(())
}
