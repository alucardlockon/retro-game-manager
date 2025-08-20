use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use eframe::{egui, App};
use rayon::prelude::*;
use walkdir::WalkDir;

mod xml;
use crate::xml::{parse_games_from_file, GameEntry};
use crate::xml::extract_game_xml_by_index;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
struct RecentFilters {
	platforms: Vec<String>,
	regions: Vec<String>,
	languages: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTab { Info, Xml }

impl RecentFilters {
	fn load() -> Self {
		if let Some(dir) = dirs::config_dir() {
			let path = dir.join("retro_game_search").join("recent.json");
			if let Ok(data) = fs::read(&path) {
				if let Ok(v) = serde_json::from_slice::<RecentFilters>(&data) {
					return v;
				}
			}
		}
		Self::default()
	}
	fn save(&self) {
		if let Some(dir) = dirs::config_dir() {
			let root = dir.join("retro_game_search");
			let _ = fs::create_dir_all(&root);
			let path = root.join("recent.json");
			if let Ok(data) = serde_json::to_vec_pretty(self) {
				let _ = fs::write(path, data);
			}
		}
	}
}

struct RetroGameSearchApp {
	query: String,
	platform_filter: String,
	region_filter: String,
	language_filter: String,
	status: String,
	index: Vec<GameEntry>,
	platforms: Vec<String>,
	available_regions: Vec<String>,
	available_languages: Vec<String>,
	recent_platforms: Vec<String>,
	recent_regions: Vec<String>,
	recent_languages: Vec<String>,
	recent_store: RecentFilters,
	xmldb_dir: PathBuf,
    // 详情页状态
    selected_index: Option<usize>,
    show_detail: bool,
    detail_xml_cache: Option<String>,
    detail_tab: DetailTab,
}

impl RetroGameSearchApp {
	fn new(xmldb_dir: PathBuf) -> Result<Self> {
		let (index, platforms, regions, languages, status) = load_index(&xmldb_dir)?;
		let persisted = RecentFilters::load();
		Ok(Self {
			query: String::new(),
			platform_filter: String::new(),
			region_filter: String::new(),
			language_filter: String::new(),
			status,
			platforms,
			available_regions: regions,
			available_languages: languages,
			recent_platforms: persisted.platforms.clone(),
			recent_regions: persisted.regions.clone(),
			recent_languages: persisted.languages.clone(),
			recent_store: persisted,
			index,
			xmldb_dir,
            selected_index: None,
            show_detail: false,
            detail_xml_cache: None,
            detail_tab: DetailTab::Info,
		})
	}

	fn persist_recents(&mut self) {
		self.recent_store.platforms = self.recent_platforms.clone();
		self.recent_store.regions = self.recent_regions.clone();
		self.recent_store.languages = self.recent_languages.clone();
		self.recent_store.save();
	}
}

impl App for RetroGameSearchApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		egui::TopBottomPanel::top("top").show(ctx, |ui| {
			ui.horizontal_wrapped(|ui| {
				ui.label("搜索");
				let _changed = ui.text_edit_singleline(&mut self.query).changed();
				ui.separator();

				// 平台：标签在左 + 下拉
				ui.horizontal(|ui| {
					ui.label("平台");
					egui::ComboBox::from_id_source("platform_combo")
						.selected_text(if self.platform_filter.is_empty() { "全部".to_string() } else { self.platform_filter.clone() })
						.show_ui(ui, |ui| {
						let mut chosen: Option<String> = None;
						if !self.recent_platforms.is_empty() {
							ui.label("最近");
							for rp in self.recent_platforms.clone() {
								let selected = !self.platform_filter.is_empty() && self.platform_filter == rp;
								if ui.selectable_label(selected, &rp).clicked() {
									self.platform_filter = rp.clone();
									chosen = Some(rp.clone());
								}
							}
							ui.separator();
						}
						if ui.selectable_label(self.platform_filter.is_empty(), "全部").clicked() {
							self.platform_filter.clear();
						}
						ui.separator();
						for p in self.platforms.clone() {
							let selected = !self.platform_filter.is_empty() && self.platform_filter == p;
							if ui.selectable_label(selected, &p).clicked() {
								self.platform_filter = p.clone();
								chosen = Some(p.clone());
							}
						}
						if let Some(p) = chosen {
							add_recent(&mut self.recent_platforms, &p);
							self.persist_recents();
						}
					});
				});
			});

			// 第二行：区域/语言下拉 + 清空按钮区
			ui.horizontal(|ui| {
				// 区域（左侧标签）
				ui.horizontal(|ui| {
					ui.label("区域");
					egui::ComboBox::from_id_source("region_combo")
						.selected_text(if self.region_filter.is_empty() { "全部".to_string() } else { self.region_filter.clone() })
						.show_ui(ui, |ui| {
						let mut chosen: Option<String> = None;
						if !self.recent_regions.is_empty() {
							ui.label("最近");
							for rr in self.recent_regions.clone() {
								let selected = !self.region_filter.is_empty() && self.region_filter == rr;
								if ui.selectable_label(selected, &rr).clicked() {
									self.region_filter = rr.clone();
									chosen = Some(rr.clone());
								}
							}
							ui.separator();
						}
						if ui.selectable_label(self.region_filter.is_empty(), "全部").clicked() {
							self.region_filter.clear();
						}
						ui.separator();
						for r in self.available_regions.clone() {
							let selected = !self.region_filter.is_empty() && self.region_filter == r;
							if ui.selectable_label(selected, &r).clicked() {
								self.region_filter = r.clone();
								chosen = Some(r.clone());
							}
						}
						if let Some(r) = chosen {
							add_recent(&mut self.recent_regions, &r);
							self.persist_recents();
						}
					});
				});

				ui.separator();

				// 语言（左侧标签）
				ui.horizontal(|ui| {
					ui.label("语言");
					egui::ComboBox::from_id_source("language_combo")
						.selected_text(if self.language_filter.is_empty() { "全部".to_string() } else { self.language_filter.clone() })
						.show_ui(ui, |ui| {
						let mut chosen: Option<String> = None;
						if !self.recent_languages.is_empty() {
							ui.label("最近");
							for rl in self.recent_languages.clone() {
								let selected = !self.language_filter.is_empty() && self.language_filter == rl;
								if ui.selectable_label(selected, &rl).clicked() {
									self.language_filter = rl.clone();
									chosen = Some(rl.clone());
								}
							}
							ui.separator();
						}
						if ui.selectable_label(self.language_filter.is_empty(), "全部").clicked() {
							self.language_filter.clear();
						}
						ui.separator();
						for l in self.available_languages.clone() {
							let selected = !self.language_filter.is_empty() && self.language_filter == l;
							if ui.selectable_label(selected, &l).clicked() {
								self.language_filter = l.clone();
								chosen = Some(l.clone());
							}
						}
						if let Some(l) = chosen {
							add_recent(&mut self.recent_languages, &l);
							self.persist_recents();
						}
					});
				});

				ui.separator();

				// 清空按钮区
				ui.separator();
				if ui.button("全部清空").clicked() {
					self.query.clear();
					self.platform_filter.clear();
					self.region_filter.clear();
					self.language_filter.clear();
				}
			});
		});

		let results = filter_results(
			&self.index,
			&self.query,
			&self.platform_filter,
			&self.region_filter,
			&self.language_filter,
		);

		egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
			ui.label(format!(
				"{} | 结果: {} 条",
				self.status,
				results.len()
			));
		});

        // 详情窗口（单独窗口显示）
        if self.show_detail {
            let mut open = true;
            if let Some(sel) = self.selected_index {
                if let Some(&g) = results.get(sel) {
                    let title = format!("{}", g.name);
                    egui::Window::new(title)
                        .open(&mut open)
                        .resizable(true)
                        .default_size(egui::vec2(720.0, 900.0))
                        .vscroll(true)
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                let info_clicked = ui.selectable_label(self.detail_tab == DetailTab::Info, "基本信息").clicked();
                                let xml_clicked = ui.selectable_label(self.detail_tab == DetailTab::Xml, "XML 源码").clicked();
                                if info_clicked { self.detail_tab = DetailTab::Info; }
                                if xml_clicked { self.detail_tab = DetailTab::Xml; }
                            });
                            ui.separator();
                            match self.detail_tab {
                                DetailTab::Info => {
                                    ui.label(format!("平台: {}", g.platform));
                                    ui.label(format!("区域: {}", g.region.as_deref().unwrap_or("未知")));
                                    ui.label(format!("语言: {}", g.languages.as_deref().unwrap_or("未知")));
                                    if let Some(a) = &g.archive_name { ui.label(format!("归档名: {}", a)); }
                                    ui.label(format!("来源文件: {}", g.file_path));
                                }
                                DetailTab::Xml => {
                                    if self.detail_xml_cache.is_none() {
                                        let p = PathBuf::from(&g.file_path);
                                        if let Ok(xml) = crate::xml::extract_game_xml_by_index(&p, g.game_idx) {
                                            self.detail_xml_cache = Some(xml);
                                        }
                                    }
                                    let mut code_txt = self.detail_xml_cache.clone().unwrap_or_else(|| "<game/>".to_string());
                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut code_txt)
                                                .code_editor()
                                                .interactive(false)
                                                .desired_width(ui.available_width())
                                        );
                                    });
                                }
                            }
                        });
                } else {
                    egui::Window::new("游戏详情")
                        .open(&mut open)
                        .resizable(true)
                        .default_size(egui::vec2(720.0, 900.0))
                        .show(ctx, |ui| {
                            ui.label("所选条目已变化");
                        });
                }
            } else {
                egui::Window::new("游戏详情")
                    .open(&mut open)
                    .resizable(true)
                    .default_size(egui::vec2(720.0, 900.0))
                    .show(ctx, |ui| {
                        ui.label("未选择条目");
                    });
            }
            if !open { self.show_detail = false; }
        }

		egui::CentralPanel::default().show(ctx, |ui| {
			egui::ScrollArea::vertical().show(ui, |ui| {
				for (i, g) in results.iter().take(500).enumerate() {
					let width = ui.available_width();
					let card_width = (width - 12.0).max(0.0);
					let inner = egui::Frame::group(ui.style()).show(ui, |ui| {
						ui.set_width(card_width);
						ui.heading(&g.name);
						ui.label(format!(
							"平台: {} | 区域: {} | 语言: {}",
							g.platform,
							g.region.as_deref().unwrap_or("未知"),
							g.languages.as_deref().unwrap_or("未知")
						));
						if let Some(archive_name) = &g.archive_name {
							ui.label(format!("归档名: {}", archive_name));
						}
					});
					let rect = inner.response.rect;
					let id = egui::Id::new(("game_card", i));
					let response = ui.interact(rect, id, egui::Sense::click());
					if response.hovered() {
						let mut color = ui.visuals().widgets.hovered.bg_fill;
						color = color.linear_multiply(0.20);
						ui.painter().rect_filled(rect, 4.0, color);
					}
					if response.clicked() {
						self.selected_index = Some(i);
						self.show_detail = true;
						self.detail_xml_cache = None;
						self.detail_tab = DetailTab::Info;
					}
					ui.add_space(4.0);
				}
				if results.len() > 500 {
					ui.label("结果过多，仅显示前 500 条。请继续缩小搜索条件。");
				}
			});
		});
	}
}

fn load_index(xmldb_dir: &Path) -> Result<(Vec<GameEntry>, Vec<String>, Vec<String>, Vec<String>, String)> {
	if !xmldb_dir.exists() {
		return Err(anyhow!("xmldb 目录不存在: {}", xmldb_dir.display()));
	}

	let mut files: Vec<PathBuf> = Vec::new();
	for entry in WalkDir::new(xmldb_dir).into_iter().filter_map(|e| e.ok()) {
		if entry.file_type().is_file() {
			let path = entry.path();
			if let Some(ext) = path.extension() {
				if ext == "xml" {
					files.push(path.to_path_buf());
				}
			}
		}
	}

	if files.is_empty() {
		return Ok((Vec::new(), Vec::new(), Vec::new(), Vec::new(), "未找到 XML 文件".to_string()));
	}

	let games: Vec<GameEntry> = files
		.par_iter()
		.filter_map(|p| parse_games_from_file(p).ok())
		.flatten()
		.collect();

	let mut platforms: Vec<String> = games
		.iter()
		.map(|g| g.platform.clone())
		.collect();
	platforms.sort_unstable();
	platforms.dedup();

	let mut regions: Vec<String> = games
		.iter()
		.filter_map(|g| g.region.as_ref().map(|s| s.trim().to_string()))
		.filter(|s| !s.is_empty())
		.collect();
	regions.sort_unstable();
	regions.dedup();

	let mut languages: Vec<String> = games
		.iter()
		.filter_map(|g| g.languages.as_ref())
		.flat_map(|s| s.split(','))
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();
	languages.sort_unstable();
	languages.dedup();

	let status = format!(
		"已索引平台 {} 个，游戏条目 {} 条",
		platforms.len(),
		games.len()
	);

	Ok((games, platforms, regions, languages, status))
}

fn filter_results<'a>(
	index: &'a [GameEntry],
	query: &str,
	platform: &str,
	region: &str,
	language: &str,
) -> Vec<&'a GameEntry> {
	let q = query.trim().to_lowercase();
	let p = platform.trim().to_lowercase();
	let r = region.trim().to_lowercase();
	let l = language.trim().to_lowercase();

	index
		.iter()
		.filter(|g| {
			let mut ok = true;
			if !q.is_empty() {
				ok &= g.name.to_lowercase().contains(&q)
					|| g.archive_name
						.as_deref()
						.map(|n| n.to_lowercase().contains(&q))
						.unwrap_or(false);
			}
			if !p.is_empty() {
				ok &= g.platform.to_lowercase().contains(&p);
			}
			if !r.is_empty() {
				ok &= g
					.region
					.as_deref()
					.map(|v| v.to_lowercase().contains(&r))
					.unwrap_or(false);
			}
			if !l.is_empty() {
				ok &= g
					.languages
					.as_deref()
					.map(|v| v.to_lowercase().contains(&l))
					.unwrap_or(false);
			}
			ok
		})
		.collect()
}

fn add_recent(list: &mut Vec<String>, value: &str) {
	if let Some(pos) = list.iter().position(|v| v == value) {
		list.remove(pos);
	}
	list.insert(0, value.to_string());
	if list.len() > 3 {
		list.truncate(3);
	}
}

fn install_chinese_fonts(ctx: &egui::Context) {
	use egui::FontFamily;
	use egui::FontId;
	use egui::TextStyle;

	let mut fonts = egui::FontDefinitions::default();

	let mut db = fontdb::Database::new();
	db.load_system_fonts();

	let candidates = [
		"PingFang SC",
		"PingFang HK",
		"PingFang TC",
		"Noto Sans CJK SC",
		"Noto Sans CJK TC",
		"Noto Sans CJK JP",
		"Source Han Sans SC",
		"Source Han Sans CN",
		"Heiti SC",
		"STHeitiSC-Medium",
		"Hiragino Sans GB",
		"Microsoft YaHei",
		"WenQuanYi Zen Hei",
	];

	for name in candidates.iter() {
		let query = fontdb::Query {
			families: &[fontdb::Family::Name(name)],
			..Default::default()
		};
		if let Some(id) = db.query(&query) {
			if let Some(face) = db.face(id) {
				let key = format!("chinese:{}", name);
				match &face.source {
					fontdb::Source::Binary(data) => {
						let bytes: Vec<u8> = data.as_ref().as_ref().to_vec();
						fonts.font_data.insert(key.clone(), egui::FontData::from_owned(bytes));
					}
					fontdb::Source::File(path) => {
						if let Ok(data) = std::fs::read(path) {
							fonts.font_data.insert(key.clone(), egui::FontData::from_owned(data));
						}
					}
					_ => {}
				}
				fonts.families.entry(FontFamily::Proportional).or_default().insert(0, key.clone());
				fonts.families.entry(FontFamily::Monospace).or_default().insert(0, key.clone());
				break;
			}
		}
	}

	let size = 14.0;
	let mut style = (*ctx.style()).clone();
	style.text_styles = [
		(TextStyle::Body, FontId::new(size, FontFamily::Proportional)),
		(TextStyle::Button, FontId::new(size, FontFamily::Proportional)),
		(TextStyle::Heading, FontId::new(size + 4.0, FontFamily::Proportional)),
		(TextStyle::Monospace, FontId::new(size, FontFamily::Monospace)),
		(TextStyle::Small, FontId::new(size - 2.0, FontFamily::Proportional)),
	]
	.into();

	ctx.set_fonts(fonts);
	ctx.set_style(style);
}

fn main() -> Result<()> {
	let xmldb_dir = std::env::current_dir()
		.context("无法获取当前目录")?
		.join("xmldb");

	let app = RetroGameSearchApp::new(xmldb_dir)?;

	let native_options = eframe::NativeOptions::default();
	eframe::run_native(
		"Retro Game Search",
		native_options,
		Box::new(move |cc| {
			install_chinese_fonts(&cc.egui_ctx);
			Box::new(app)
		}),
	)
	.map_err(|e| anyhow!(e.to_string()))?;

	Ok(())
}
