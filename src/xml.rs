use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;

#[derive(Debug, Clone)]
pub struct GameEntry {
    pub platform: String,
    pub name: String,
    pub archive_name: Option<String>,
    pub region: Option<String>,
    pub languages: Option<String>,
    pub file_path: String,
    pub game_idx: usize,
}

pub fn parse_games_from_file(path: &Path) -> Result<Vec<GameEntry>> {
    let platform = infer_platform_from_filename(path).unwrap_or_else(|| "Unknown".to_string());

    let mut reader =
        Reader::from_file(path).with_context(|| format!("读取 XML 失败: {}", path.display()))?;
    reader.trim_text(true);

    let mut buf = Vec::new();

    let mut in_game = false;
    let mut current_game_name: Option<String> = None;
    let mut current_archive_region: Option<String> = None;
    let mut current_archive_languages: Option<String> = None;
    let mut current_archive_name: Option<String> = None;
    let mut current_game_region: Option<String> = None;
    let mut current_game_languages: Option<String> = None;
    let mut current_details_region: Option<String> = None;
    let mut game_idx_counter: usize = 0;

    let mut results: Vec<GameEntry> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name() {
                QName(b"game") => {
                    in_game = true;
                    for a in e.attributes().flatten() {
                        if a.key == QName(b"name") {
                            current_game_name = a
                                .decode_and_unescape_value(&reader)
                                .ok()
                                .map(|c| c.into_owned());
                        } else if a.key == QName(b"region") {
                            current_game_region = a
                                .decode_and_unescape_value(&reader)
                                .ok()
                                .map(|c| c.into_owned());
                        } else if a.key == QName(b"languages") {
                            current_game_languages = a
                                .decode_and_unescape_value(&reader)
                                .ok()
                                .map(|c| c.into_owned());
                        }
                    }
                    current_archive_region = None;
                    current_archive_languages = None;
                    current_archive_name = None;
                    current_details_region = None;
                }
                n if n == QName(b"archive") && in_game => {
                    for a in e.attributes().flatten() {
                        if a.key == QName(b"region") {
                            current_archive_region = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        } else if a.key == QName(b"languages") {
                            current_archive_languages = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        } else if a.key == QName(b"name") {
                            current_archive_name = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }
                QName(b"details") if in_game => {
                    if current_archive_region.is_none() && current_game_region.is_none() {
                        for a in e.attributes().flatten() {
                            if a.key == QName(b"region") {
                                current_details_region = a
                                    .decode_and_unescape_value(&reader)
                                    .ok()
                                    .map(|c| c.into_owned());
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name() {
                QName(b"archive") if in_game => {
                    for a in e.attributes().flatten() {
                        if a.key == QName(b"region") {
                            current_archive_region = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        } else if a.key == QName(b"languages") {
                            current_archive_languages = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        } else if a.key == QName(b"name") {
                            current_archive_name = Some(
                                a.decode_and_unescape_value(&reader)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }
                QName(b"details") if in_game => {
                    if current_archive_region.is_none() && current_game_region.is_none() {
                        for a in e.attributes().flatten() {
                            if a.key == QName(b"region") {
                                current_details_region = a
                                    .decode_and_unescape_value(&reader)
                                    .ok()
                                    .map(|c| c.into_owned());
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => {
                if e.name() == QName(b"game") {
                    if let Some(name) = current_game_name.take() {
                        let merged_region = current_archive_region
                            .take()
                            .or(current_game_region.take())
                            .or(current_details_region.take());
                        let merged_languages = current_archive_languages
                            .take()
                            .or(current_game_languages.take());

                        results.push(GameEntry {
                            platform: platform.clone(),
                            name,
                            archive_name: current_archive_name.take(),
                            region: merged_region,
                            languages: merged_languages,
                            file_path: path.display().to_string(),
                            game_idx: game_idx_counter,
                        });
                        game_idx_counter += 1;
                    }
                    in_game = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(err).with_context(|| format!("解析失败: {}", path.display()));
            }
            _ => {}
        }
    }

    Ok(results)
}

fn infer_platform_from_filename(path: &Path) -> Option<String> {
    let fname = path.file_stem()?.to_string_lossy();
    let s = fname.split(" (").next().unwrap_or(&fname);
    Some(s.to_string())
}

pub fn extract_game_xml_by_index(path: &Path, target_idx: usize) -> Result<String> {
    use quick_xml::Writer;
    use std::io::Cursor;

    let mut reader =
        Reader::from_file(path).with_context(|| format!("读取 XML 失败: {}", path.display()))?;
    reader.trim_text(false);
    let mut buf = Vec::new();

    let mut idx: usize = 0;
    let mut capturing = false;
    let mut depth: i32 = 0;
    let mut output: Vec<u8> = Vec::new();
    let mut writer = Writer::new(Cursor::new(&mut output));

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name() == QName(b"game") {
                    if idx == target_idx {
                        capturing = true;
                        depth = 1;
                        writer.write_event(Event::Start(e.to_owned())).ok();
                    } else {
                        idx += 1;
                    }
                } else if capturing {
                    depth += 1;
                    writer.write_event(Event::Start(e.to_owned())).ok();
                }
            }
            Ok(Event::Empty(e)) => {
                if e.name() == QName(b"game") {
                    if idx == target_idx {
                        writer.write_event(Event::Empty(e.to_owned())).ok();
                        break;
                    } else {
                        idx += 1;
                    }
                } else if capturing {
                    writer.write_event(Event::Empty(e.to_owned())).ok();
                }
            }
            Ok(Event::Text(e)) => {
                if capturing {
                    writer.write_event(Event::Text(e)).ok();
                }
            }
            Ok(Event::CData(e)) => {
                if capturing {
                    writer.write_event(Event::CData(e)).ok();
                }
            }
            Ok(Event::Comment(e)) => {
                if capturing {
                    writer.write_event(Event::Comment(e)).ok();
                }
            }
            Ok(Event::End(e)) => {
                if capturing {
                    writer.write_event(Event::End(e.to_owned())).ok();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if e.name() == QName(b"game") {
                    // shouldn't happen when not capturing as we increase idx on Start/Empty
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    let s = String::from_utf8_lossy(&output).to_string();
    Ok(s)
}
