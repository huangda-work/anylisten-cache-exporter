use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub fn read_lyrics(database_path: &Path, music_id: &str) -> Result<Option<String>, String> {
    let database = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    let mut statement = database
        .prepare(
            "SELECT lyric FROM lyric WHERE id = ?1 AND type IN ('edited', 'raw') \
             ORDER BY CASE type WHEN 'edited' THEN 0 ELSE 1 END",
        )
        .map_err(|error| error.to_string())?;
    if let Some(lyrics) = query_lyrics(&mut statement, music_id)? {
        return Ok(Some(lyrics));
    }
    if let Some((base, suffix)) = music_id.rsplit_once('_') {
        if suffix
            .strip_suffix('k')
            .is_some_and(|rate| !rate.is_empty() && rate.chars().all(|ch| ch.is_ascii_digit()))
        {
            return query_lyrics(&mut statement, base);
        }
    }
    Ok(None)
}

fn query_lyrics(
    statement: &mut rusqlite::Statement<'_>,
    music_id: &str,
) -> Result<Option<String>, String> {
    let rows = statement
        .query_map([music_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    for row in rows {
        let stored = row.map_err(|error| error.to_string())?;
        if let Some(first_line) = stored.lines().next() {
            if let Ok(text) = serde_json::from_str::<String>(first_line) {
                if !text.trim().is_empty() {
                    return Ok(Some(text));
                }
            }
        }
    }
    Ok(None)
}
