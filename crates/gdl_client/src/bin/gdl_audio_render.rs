//! Offline render of the simulation's audio requests (M17 research; not
//! part of the game). Reads a game-run frame replay made with
//! GDL_REPLAY_EVENTS (its `updates[k].events`), feeds each update's events
//! to the native mixer exactly as gdl_play's play_audio does, renders one
//! update's worth of output (two vblanks at 59.94 Hz) after each, and
//! writes interleaved stereo f32 at 48 kHz.
//!
//!   gdl_audio_render CONFIG.json REPLAY.json NEW_OUT.f32 [music|sfx|all] [VOL_SFX]
//!
//! CONFIG is the play configuration (its "audio" block: rom, dcs, banks,
//! streams). `music` keeps only the stream events, `sfx` only the sounds.
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("usage: gdl_audio_render CONFIG.json REPLAY.json NEW_OUT.f32 [music|sfx|all] [VOL_SFX]".into());
    }
    // SAFETY: single-threaded before any audio thread exists.
    unsafe { std::env::set_var("GDL_NO_AUDIO", "1") };
    let cfg: Value = serde_json::from_slice(&gdl_client::bounded_read(Path::new(&args[0]))?)?;
    let a = &cfg["audio"];
    let rom = gdl_formats::audio::AudioRom::parse(&gdl_client::bounded_read(Path::new(a["rom"].as_str().ok_or("audio rom")?))?)?;
    let paths = |k: &str| -> HashMap<String, PathBuf> {
        a[k].as_object().map_or(HashMap::new(), |m| m.iter().filter_map(|(n, v)| Some((n.to_uppercase(), v.as_str()?.into()))).collect())
    };
    let dcs = match a["dcs"].as_str() {
        Some(p) => Some(gdl_client::bounded_read(Path::new(p))?),
        None => None,
    };
    let mut out = gdl_client::audio::AudioOut::new(rom, paths("banks"), paths("streams"), dcs.as_deref());
    out.set_synchronous(true);
    let which = args.get(3).map(String::as_str).unwrap_or("all");
    let vol_sfx: i32 = args.get(4).map(|v| v.parse()).transpose()?.unwrap_or(128);
    out.set_options(vol_sfx, 1);
    let replay: Value = serde_json::from_slice(&gdl_client::bounded_read(Path::new(&args[1]))?)?;
    let updates = replay["results"].as_array().or_else(|| replay["updates"].as_array()).ok_or("replay has no per-update list")?;
    let mut pcm: Vec<f32> = Vec::new();
    // One update = two vblanks: 2 × 48000 / 59.94 frames, carried fractionally.
    let per = 2.0 * 48000.0 / 59.94;
    let mut due = 0.0f64;
    for u in updates {
        for e in u["events"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let (kind, body) = match e {
                Value::Object(m) if m.len() == 1 => {
                    let (k, v) = m.iter().next().expect("one key");
                    (k.as_str(), v)
                }
                Value::String(s) => (s.as_str(), &Value::Null),
                _ => continue,
            };
            let i = |k: &str| body[k].as_i64().unwrap_or(0) as i32;
            let music = matches!(kind, "Music" | "MusicVolume" | "MusicStop");
            let sound = matches!(kind, "Sound" | "LoopSound" | "StopSound" | "ItemSound" | "ItemSoundStop");
            if (which == "music" && !music) || (which == "sfx" && !sound) || !(music || sound) {
                continue;
            }
            match kind {
                "Sound" => out.play(i("id"), i("volume"), i("pan"), i("priority")),
                "LoopSound" => out.play_loop(i("id"), i("volume"), i("pan"), i("priority")),
                "StopSound" => out.stop(i("id")),
                "ItemSound" => out.play_tagged(i("slot") as u32, i("id"), i("volume"), i("pan"), i("priority")),
                "ItemSoundStop" => out.stop_tagged(i("slot") as u32),
                "Music" => {
                    out.stream(body["name"].as_str().unwrap_or(""), body["looping"].as_bool().unwrap_or(false), i("volume"));
                }
                "MusicVolume" => out.stream_volume(i("volume")),
                "MusicStop" => out.stream_stop(),
                _ => {}
            }
        }
        due += per;
        let n = due.floor() as usize;
        due -= n as f64;
        pcm.extend(out.render_offline(n));
    }
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
    f.write_all(&pcm.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())?;
    println!("{} frames from {} updates", pcm.len() / 2, updates.len());
    Ok(())
}
