use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use eframe::{egui, App, Error};
use rayon::prelude::*;
use walkdir::WalkDir;
use rfd::FileDialog;

mod xml;
mod image_loader;
mod baidu_fallback;
use crate::xml::{parse_games_from_file, GameEntry};
use crate::image_loader::{ImageLoader, ImageLoadResult};
use egui::Color32;

// 关键词高亮辅助
fn tokenize_query(q: &str) -> Vec<String> {
	q.split_whitespace()
		.filter(|s| !s.is_empty())
		.map(|s| s.to_lowercase())
		.collect()
}

fn build_highlight_job(text: &str, tokens: &[String], style: &egui::Style) -> egui::text::LayoutJob {
	use egui::text::LayoutJob;
	use egui::TextFormat;
	use egui::TextStyle;
	let mut job = LayoutJob::default();
	let font_id = TextStyle::Heading.resolve(style);
	let normal = TextFormat { font_id: font_id.clone(), color: style.visuals.text_color(), ..Default::default() };
	let highlight = TextFormat { font_id, color: style.visuals.hyperlink_color, ..Default::default() };

	if tokens.is_empty() {
		job.append(text, 0.0, normal);
		return job;
	}

	let lower = text.to_lowercase();
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	for t in tokens {
		let mut start = 0usize;
		while !t.is_empty() && start < lower.len() {
			if let Some(pos) = lower[start..].find(t) {
				let s = start + pos;
				let e = s + t.len();
				ranges.push((s, e));
				start = e;
			} else { break; }
		}
	}
	if ranges.is_empty() {
		job.append(text, 0.0, normal);
		return job;
	}
	// 合并重叠
	ranges.sort_by_key(|r| r.0);
	let mut merged: Vec<(usize, usize)> = Vec::new();
	for (s, e) in ranges {
		if let Some(last) = merged.last_mut() {
			if s <= last.1 { last.1 = last.1.max(e); continue; }
		}
		merged.push((s, e));
	}
	// 输出
	let bytes = text.as_bytes();
	let mut cursor = 0usize;
	for (s, e) in merged {
		if cursor < s {
			let seg = std::str::from_utf8(&bytes[cursor..s]).unwrap_or("");
			job.append(seg, 0.0, normal.clone());
		}
		let seg = std::str::from_utf8(&bytes[s..e]).unwrap_or("");
		job.append(seg, 0.0, highlight.clone());
		cursor = e;
	}
	if cursor < text.len() {
		let seg = std::str::from_utf8(&bytes[cursor..]).unwrap_or("");
		job.append(seg, 0.0, normal);
	}
	job
}

// XML 语法高亮（简易）
#[inline]
fn is_space(b: u8) -> bool { b.is_ascii_whitespace() }

#[inline]
fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'-' || b == b'.'
}

fn xml_highlight_job(ui: &egui::Ui, text: &str) -> egui::text::LayoutJob {
	use egui::text::LayoutJob;
	use egui::TextFormat;
	use egui::TextStyle;
	let mut job = LayoutJob::default();
	let font_id = TextStyle::Monospace.resolve(ui.style());
	let fg = ui.style().visuals.text_color();
	let normal = TextFormat { font_id: font_id.clone(), color: fg, ..Default::default() };
	let tag_color = ui.style().visuals.hyperlink_color;
	let attr_color = ui.style().visuals.strong_text_color();
	let val_color = Color32::from_rgb(156, 220, 254);
	let comment_color = Color32::from_gray(120);

	let bytes = text.as_bytes();
	let mut i = 0usize;
	while i < bytes.len() {
		// 注释 <!-- -->
		if bytes[i..].starts_with(b"<!--") {
			if let Some(end) = find_bytes(&bytes, i + 4, b"-->") {
				let seg = std::str::from_utf8(&bytes[i..end+3]).unwrap_or("");
				job.append(seg, 0.0, TextFormat { font_id: font_id.clone(), color: comment_color, ..Default::default() });
				i = end + 3;
				continue;
			} else {
				let seg = std::str::from_utf8(&bytes[i..]).unwrap_or("");
				job.append(seg, 0.0, TextFormat { font_id: font_id.clone(), color: comment_color, ..Default::default() });
				break;
			}
		}
		// 标签 <...>
		if bytes[i] == b'<' {
			// 输出 '<'
			job.append("<", 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() });
			i += 1;
			// 可能有 '/'
			if i < bytes.len() && bytes[i] == b'/' {
				job.append("/", 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() });
				i += 1;
			}
			// 读取标签名
			let start = i;
			while i < bytes.len() && is_name_char(bytes[i]) { i += 1; }
			if i > start {
				let seg = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
				job.append(seg, 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() });
			}
			// 属性
			loop {
				// 跳过空白
				while i < bytes.len() && is_space(bytes[i]) { i += 1; }
				if i >= bytes.len() { break; }
				if bytes[i] == b'>' { job.append(">", 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() }); i += 1; break; }
				if bytes[i] == b'/' {
					job.append("/", 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() });
					if i + 1 < bytes.len() && bytes[i+1] == b'>' {
						job.append(">", 0.0, TextFormat { font_id: font_id.clone(), color: tag_color, ..Default::default() });
						i += 2; break;
					} else { i += 1; continue; }
				}
				// 属性名
				let an_start = i;
				while i < bytes.len() && is_name_char(bytes[i]) { i += 1; }
				if i > an_start {
					let seg = std::str::from_utf8(&bytes[an_start..i]).unwrap_or("");
					job.append(seg, 0.0, TextFormat { font_id: font_id.clone(), color: attr_color, ..Default::default() });
				}
				// 跳过空白
				while i < bytes.len() && is_space(bytes[i]) { i += 1; }
				if i < bytes.len() && bytes[i] == b'=' { job.append("=", 0.0, normal.clone()); i += 1; }
				while i < bytes.len() && is_space(bytes[i]) { i += 1; }
				// 值（引号）
				if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
					let quote = bytes[i];
					let vstart = i;
					i += 1;
					while i < bytes.len() && bytes[i] != quote { i += 1; }
					let vend = if i < bytes.len() { i + 1 } else { i };
					let seg = std::str::from_utf8(&bytes[vstart..vend]).unwrap_or("");
					job.append(seg, 0.0, TextFormat { font_id: font_id.clone(), color: val_color, ..Default::default() });
					if i < bytes.len() { i += 1; }
				}
			}
			continue;
		}
		// 普通文本：直到下一个 '<'
		let start = i;
		while i < bytes.len() && bytes[i] != b'<' { i += 1; }
		let seg = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
		job.append(seg, 0.0, normal.clone());
	}
	job
}

fn find_bytes(hay: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
	let mut i = start;
	while i + needle.len() <= hay.len() {
		if &hay[i..i+needle.len()] == needle { return Some(i); }
		i += 1;
	}
	None
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
struct RecentFilters {
	platforms: Vec<String>,
	regions: Vec<String>,
	languages: Vec<String>,
	selected_platforms: Vec<String>,  // 添加记住选择的平台
	selected_region: Option<String>,   // 添加记住选择的区域
	selected_language: Option<String>, // 添加记住选择的语言
	default_vendors: String,           // 添加默认厂商列表
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTab { Info, Xml, WebSearch }

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

struct RetroGameManagerApp {
	query: String,
	platform_filters: Vec<String>,
	platform_search: String,
	show_platform_selector: bool,
	show_preferences: bool,
	show_about: bool,
	pending_file_rename: Option<(std::path::PathBuf, GameEntry)>,
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
    // 配置选项
    default_vendors: String,
    // 详情页状态
    selected_index: Option<usize>,
    show_detail: bool,
    detail_xml_cache: Option<String>,
    detail_tab: DetailTab,
    // 图片加载器
    image_loader: Arc<ImageLoader>,
    // 初始化标志
    initialized: bool,
}

impl RetroGameManagerApp {
	fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
		let xmldb_dir = std::env::current_dir()
			.context("无法获取当前目录")?
			.join("xmldb");
		let (index, platforms, regions, languages, status) = load_index(&xmldb_dir)?;
		let persisted = RecentFilters::load();
		install_chinese_fonts(&cc.egui_ctx);
		let image_loader = Arc::new(ImageLoader::new());
		Ok(Self {
			query: String::new(),
			platform_filters: persisted.selected_platforms.clone(),
			platform_search: String::new(),
			show_platform_selector: false,
			show_preferences: false,
			show_about: false,
			pending_file_rename: None,
			region_filter: persisted.selected_region.clone().unwrap_or_default(),
			language_filter: persisted.selected_language.clone().unwrap_or_default(),
			default_vendors: persisted.default_vendors.clone(),
			status,
			platforms,
			available_regions: regions,
			available_languages: languages,
			recent_platforms: persisted.platforms.clone(),
			recent_regions: persisted.regions.clone(),
			recent_languages: persisted.languages.clone(),
			recent_store: persisted,
			index,
			selected_index: None,
			show_detail: false,
			detail_xml_cache: None,
			detail_tab: DetailTab::Info,
			image_loader, // 初始化图片加载器
			initialized: false,
		})
	}

	fn persist_recents(&mut self) {
		self.recent_store.platforms = self.recent_platforms.clone();
		self.recent_store.regions = self.recent_regions.clone();
		self.recent_store.languages = self.recent_languages.clone();
		self.recent_store.selected_platforms = self.platform_filters.clone();  // 保存当前选择的平台
		self.recent_store.selected_region = if self.region_filter.is_empty() { 
			None 
		} else { 
			Some(self.region_filter.clone()) 
		};  // 保存当前选择的区域
		self.recent_store.selected_language = if self.language_filter.is_empty() { 
			None 
		} else { 
			Some(self.language_filter.clone()) 
		};  // 保存当前选择的语言
		
		// 保存常用平台配置
		self.recent_store.default_vendors = self.default_vendors.clone();
		
		self.recent_store.save();
	}
}

impl App for RetroGameManagerApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// 初始化逻辑
		if !self.initialized {
			// 注意：我们不再自动选中常用平台
			// 平台选择应该完全从保存的数据中恢复
			// self.platform_filters 已经在 new() 函数中从 persisted.selected_platforms 恢复了
			self.initialized = true;
		}
		
		// 在主窗口内显示菜单栏，按照Mac标准排列
		egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
			egui::menu::bar(ui, |ui| {
				// Mac标准菜单排列：首选项、关于
				if ui.button("首选项").clicked() {
					self.show_preferences = true;
				}
				if ui.button("关于").clicked() {
					self.show_about = true;
				}
			});
		});
		
		// 主要搜索界面
		egui::TopBottomPanel::top("search").show(ctx, |ui| {
			ui.horizontal_wrapped(|ui| {
				ui.label("搜索");
				let _changed = ui.text_edit_singleline(&mut self.query).changed();
				ui.separator();

				// 添加键盘快捷键提示
				// Mac用户可以使用 Cmd + , 打开首选项
				
				// 平台：标签在左 + 多选
				ui.horizontal(|ui| {
					ui.label("平台");
					// 显示已选择的平台数量
					let selected_count = self.platform_filters.len();
					let display_text = if selected_count == 0 {
						"未选择".to_string()
					} else if selected_count == self.platforms.len() {
						"全部".to_string()
					} else {
						format!("已选择 {} 项", selected_count)
					};
					
					// 使用按钮触发平台选择窗口
					let button_response = ui.button(display_text);
					
					if button_response.clicked() {
						self.show_platform_selector = true;
					}
					
					// 显示平台选择窗口
					if self.show_platform_selector {
						let mut open = true;
						egui::Window::new("选择平台")
							.open(&mut open)
							.resizable(true)
							.default_size(egui::vec2(350.0, 400.0))
							.default_pos(button_response.rect.left_bottom())  // 设置窗口位置在按钮下方
							.show(ui.ctx(), |ui| {
								// 添加平台搜索框
								ui.horizontal(|ui| {
									ui.label("搜索:");
									ui.text_edit_singleline(&mut self.platform_search);
								});
								
								// 添加"全选"选项
								let all_selected = self.platform_filters.len() == self.platforms.len();
								let mut new_all_selected = all_selected;
								if ui.checkbox(&mut new_all_selected, "全选").clicked() {
									if new_all_selected {
										self.platform_filters = self.platforms.clone();
									} else {
										self.platform_filters.clear();
									}
									self.persist_recents();
								}
								ui.separator();
								
								// 使用ScrollArea来容纳平台列表，避免窗口太高
								egui::ScrollArea::vertical()
									.max_height(300.0)
									.show(ui, |ui| {
										// 添加常用平台分组
										if !self.default_vendors.is_empty() {
											ui.collapsing("常用平台", |ui| {
												// 解析自定义厂商列表
												let vendors: Vec<String> = self.default_vendors.split(',')
													.map(|s| s.trim().to_string())
													.filter(|s| !s.is_empty())
													.collect();
												
												// 查找匹配的常用平台
												let mut common_platforms = Vec::new();
												for platform in &self.platforms {
													for vendor in &vendors {
														if platform.starts_with(vendor) {
															common_platforms.push(platform.clone());
															break;
														}
													}
												}
												
												// 为每个常用平台添加checkbox
												if !common_platforms.is_empty() {
													let mut updates = Vec::new();
													for platform in &common_platforms {
														let mut selected = self.platform_filters.contains(platform);
														if ui.checkbox(&mut selected, platform).clicked() {
															updates.push((platform.clone(), selected));
														}
													}
													
													// 应用更新
													for (platform, selected) in updates {
														if selected {
															if !self.platform_filters.contains(&platform) {
																self.platform_filters.push(platform.clone());
																add_recent(&mut self.recent_platforms, &platform);
																self.persist_recents();
															}
														} else {
															self.platform_filters.retain(|p| p != &platform);
															self.persist_recents();
														}
													}
												} else {
													ui.label("未找到匹配的常用平台");
												}
											});
											
											ui.separator();
										}
										
										// 为每个平台添加checkbox，但限制显示数量
										let mut displayed_count = 0;
										let max_display = 50; // 限制最多显示50个平台
										let mut updates = Vec::new();
										
										for platform in &self.platforms {
											// 如果有搜索过滤器，只显示匹配的平台
											if !self.platform_search.is_empty() && !platform.to_lowercase().contains(&self.platform_search.to_lowercase()) {
												continue;
											}
											
											// 限制显示数量以避免卡顿
											if displayed_count >= max_display {
												ui.label(format!("... 还有 {} 个平台未显示", self.platforms.len() - displayed_count));
												break;
											}
											
											let mut selected = self.platform_filters.contains(platform);
											if ui.checkbox(&mut selected, platform).clicked() {
												updates.push((platform.clone(), selected));
											}
											displayed_count += 1;
										}
										
										// 应用更新
										let mut needs_persist = false;
										for (platform, selected) in updates {
											if selected {
												if !self.platform_filters.contains(&platform) {
													self.platform_filters.push(platform.clone());
													add_recent(&mut self.recent_platforms, &platform);
													needs_persist = true;
												}
											} else {
												self.platform_filters.retain(|p| p != &platform);
											}
										}
										
										// 如果有更改，保存到最近使用列表
										if needs_persist {
											self.persist_recents();
										}
										
										// 如果搜索过滤后没有显示任何平台，显示提示信息
										if displayed_count == 0 && !self.platform_search.is_empty() {
											ui.label("未找到匹配的平台");
										}
									});
							});
						
						// 如果窗口被关闭，更新状态
						if !open {
							self.show_platform_selector = false;
						}
					}
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
					self.platform_filters.clear();
					self.region_filter.clear();
					self.language_filter.clear();
				}
			});
		});

		let results = filter_results(
		&self.index,
		&self.query,
		&self.platform_filters,  // 传递平台过滤器数组
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
                                let web_search_clicked = ui.selectable_label(self.detail_tab == DetailTab::WebSearch, "网页搜索").clicked();
                                if info_clicked { self.detail_tab = DetailTab::Info; }
                                if xml_clicked { self.detail_tab = DetailTab::Xml; }
                                if web_search_clicked { self.detail_tab = DetailTab::WebSearch; }
                            });
                            ui.separator();
                            match self.detail_tab {
                                DetailTab::Info => {
                                    // 显示游戏详情
                                    ui.heading(&g.name);
                                    
                                    // 加载并显示图片
                                    let (boxart, title, snap) = self.image_loader.load_game_images_async(
                                        ctx,
                                        g.platform.clone(),
                                        g.name.clone(), // 使用游戏名加载图片
                                    );
                                    
                                    // 只有当至少有一张图片加载成功时，才显示图片行
                                    let has_loaded_image = matches!(boxart, ImageLoadResult::Loaded(_)) || 
                                                           matches!(title, ImageLoadResult::Loaded(_)) || 
                                                           matches!(snap, ImageLoadResult::Loaded(_));
                                    
                                    if has_loaded_image {
                                        // 显示图片，不再均分，而是保持原始宽高比并限制最大尺寸
                                        ui.horizontal(|ui| {
                                            let max_size = egui::Vec2::new(150.0, 150.0); // 限制图片的最大宽度和高度为150
                                            
                                            // 创建一个没有内边距的Frame来包裹图片
                                            let frame = egui::Frame::none();
                                            
                                            if let ImageLoadResult::Loaded(texture) = &boxart {
                                                let texture_size = texture.size();
                                                let scale = (max_size.x / texture_size[0] as f32).min(max_size.y / texture_size[1] as f32).min(1.0);
                                                let image_size = egui::Vec2::new(
                                                    texture_size[0] as f32 * scale,
                                                    texture_size[1] as f32 * scale,
                                                );
                                                frame.show(ui, |ui| {
                                                    ui.image((texture.id(), image_size));
                                                });
                                            }
                                            if let ImageLoadResult::Loaded(texture) = &title {
                                                let texture_size = texture.size();
                                                let scale = (max_size.x / texture_size[0] as f32).min(max_size.y / texture_size[1] as f32).min(1.0);
                                                let image_size = egui::Vec2::new(
                                                    texture_size[0] as f32 * scale,
                                                    texture_size[1] as f32 * scale,
                                                );
                                                frame.show(ui, |ui| {
                                                    ui.image((texture.id(), image_size));
                                                });
                                            }
                                            if let ImageLoadResult::Loaded(texture) = &snap {
                                                let texture_size = texture.size();
                                                let scale = (max_size.x / texture_size[0] as f32).min(max_size.y / texture_size[1] as f32).min(1.0);
                                                let image_size = egui::Vec2::new(
                                                    texture_size[0] as f32 * scale,
                                                    texture_size[1] as f32 * scale,
                                                );
                                                frame.show(ui, |ui| {
                                                    ui.image((texture.id(), image_size));
                                                });
                                            }
                                        });
                                    }
                                    
                                    // 添加重命名文件按钮和用归档名称重命名按钮
                                    ui.horizontal(|ui| {
                                        if ui.button("重命名文件").clicked() {
                                            // 使用rfd打开文件选择对话框
                                            if let Some(_file_path) = FileDialog::new().pick_file() {
                                                // 存储待重命名的文件和游戏
                                                self.pending_file_rename = Some((_file_path, (*g).clone()));
                                            }
                                        }
                                        
                                        // 添加一个根据归档名称进行重命名的按钮
                                        if let Some(archive_name) = &g.archive_name {
                                            if ui.button("用归档名称重命名").clicked() {
                                                // 使用rfd打开文件选择对话框
                                                if let Some(_file_path) = FileDialog::new().pick_file() {
                                                    // 创建一个带有归档名称的游戏条目副本，用于重命名
                                                    let mut game_with_archive_name = g.clone();
                                                    game_with_archive_name.name = archive_name.clone();
                                                    // 存储待重命名的文件和带有归档名称的游戏条目
                                                    self.pending_file_rename = Some((_file_path, game_with_archive_name));
                                                }
                                            }
                                        }
                                    });
                                    // 使用更小的间距
                                    ui.add_space(5.0);
                                    
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
                                    let code_txt = self.detail_xml_cache.clone().unwrap_or_else(|| "<game/>".to_string());
                                    egui::ScrollArea::both() // 启用水平和垂直滚动
                                        .auto_shrink([false, true])
                                        .max_height(ui.available_height() - 10.0)
                                        .show(ui, |ui| {
                                            let mut layouter = |_ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                                let job = xml_highlight_job(_ui, text);
                                                _ui.fonts(|f| f.layout_job(job))
                                            };
                                            ui.add(
                                                egui::TextEdit::multiline(&mut code_txt.clone())
                                                    .code_editor()
                                                    .interactive(false)
                                                    .layouter(&mut layouter)
                                                    .desired_width(ui.available_width())
                                            );
                                        });
                                }
                                DetailTab::WebSearch => {
                                    // 确定用于搜索的名称：优先使用归档名，如果没有则使用游戏名
                                    let search_name = g.archive_name.as_ref().unwrap_or(&g.name);
                                    
                                    ui.label("在浏览器中打开以下搜索链接:");
                                    ui.separator();
                                    
                                    // 百度搜索链接
                                    let baidu_url = format!("https://www.baidu.com/s?wd={}", search_name);
                                    if ui.button("🔍 百度搜索").clicked() {
                                        // 尝试在浏览器中打开链接
                                        if let Err(e) = webbrowser::open(&baidu_url) {
                                            eprintln!("无法在浏览器中打开链接: {}", e);
                                        }
                                    }
                                    ui.hyperlink_to("在浏览器中打开", &baidu_url);
                                    ui.label(&baidu_url);
                                    ui.separator();
                                    
                                    // Wikipedia搜索链接
                                    let wikipedia_url = format!("https://en.wikipedia.org/w/index.php?search={}&title=Special%3ASearch&ns0=1", search_name.replace(" ", "_"));
                                    if ui.button("🔍 Wikipedia搜索").clicked() {
                                        // 尝试在浏览器中打开链接
                                        if let Err(e) = webbrowser::open(&wikipedia_url) {
                                            eprintln!("无法在浏览器中打开链接: {}", e);
                                        }
                                    }
                                    ui.hyperlink_to("在浏览器中打开", &wikipedia_url);
                                    ui.label(&wikipedia_url);
                                    ui.separator();
                                    
                                    // Google搜索链接
                                    let google_url = format!("https://www.google.com/search?q={}", search_name);
                                    if ui.button("🔍 Google搜索").clicked() {
                                        // 尝试在浏览器中打开链接
                                        if let Err(e) = webbrowser::open(&google_url) {
                                            eprintln!("无法在浏览器中打开链接: {}", e);
                                        }
                                    }
                                    ui.hyperlink_to("在浏览器中打开", &google_url);
                                    ui.label(&google_url);
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
						let tokens = tokenize_query(&self.query);
						let job = build_highlight_job(&g.name, &tokens, ui.style());
						ui.label(job);
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
		
		// 显示首选项配置窗口
		if self.show_preferences {
			let mut open = true;
			egui::Window::new("首选项")
				.open(&mut open)
				.resizable(false)
				.default_size(egui::vec2(400.0, 250.0))
				.show(ctx, |ui| {
					ui.vertical(|ui| {
						ui.label("常用平台厂商 (逗号分隔):");
						ui.text_edit_singleline(&mut self.default_vendors);
						
						ui.separator();
						
						if ui.button("保存").clicked() {
							// 保存配置到recent_store
							self.persist_recents();
						}
						
						if ui.button("取消").clicked() {
							self.show_preferences = false;
						}
					});
				});
			
			if !open {
				self.show_preferences = false;
			}
		}
		
		// 显示关于窗口
		if self.show_about {
			let mut open = true;
			egui::Window::new("关于 retro-game-manager")
				.open(&mut open)
				.resizable(false)
				.default_size(egui::vec2(300.0, 200.0))
				.show(ctx, |ui| {
					ui.vertical_centered(|ui| {
						ui.heading("RetroGameManager");
						ui.label("版本 1.0.0");
						ui.separator();
						ui.label("一个用于管理和搜索复古游戏的工具");
						ui.label("");
						ui.label("© 2025 alucardlockon. 保留所有权利。");
						ui.label("");
						ui.hyperlink_to("GitHub 仓库", "https://github.com/yourusername/retro-game-manager");
					});
				});
			
			if !open {
				self.show_about = false;
			}
		}
		
		// 处理文件重命名
		if let Some((file_path, game)) = self.pending_file_rename.take() {
			if let Err(e) = self.rename_file_to_game_name(&file_path, &game) {
				// 显示错误消息（在实际应用中可能需要更好的错误处理）
				eprintln!("重命名文件失败: {}", e);
			}
		}
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
	platforms: &[String],  // 支持多选
	region: &str,
	language: &str,
) -> Vec<&'a GameEntry> {
	let q = query.trim().to_lowercase();
	let r = region.trim().to_lowercase();
	let l = language.trim().to_lowercase();
	
	// 创建平台过滤器的HashSet以提高查找效率
	let platform_set: std::collections::HashSet<&String> = platforms.iter().collect();

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
			// 平台：支持多选（使用HashSet提高效率）
			if !platforms.is_empty() {
				ok &= platform_set.contains(&g.platform);
			}
			// 区域：仍然模糊匹配
			if !r.is_empty() {
				ok &= g
					.region
					.as_deref()
					.map(|v| v.to_lowercase().contains(&r))
					.unwrap_or(false);
			}
			// 语言：严格匹配（不区分大小写）；支持逗号分隔多值
			if !l.is_empty() {
				ok &= g
					.languages
					.as_deref()
					.map(|v| v.split(',').map(|s| s.trim().to_lowercase()).any(|tok| tok == l))
					.unwrap_or(false);
			}
			ok
		})
		.take(1000) // 限制结果数量以避免卡顿
		.collect()
}

impl RetroGameManagerApp {
	// 重命名文件为游戏名称
	fn rename_file_to_game_name(&self, file_path: &Path, game: &GameEntry) -> Result<()> {
		// 获取文件的父目录
		let parent_dir = file_path.parent()
			.ok_or_else(|| anyhow!("无法获取文件父目录"))?;
		
		// 获取文件扩展名
		let extension = file_path.extension()
			.map(|ext| format!(".{}", ext.to_string_lossy()))
			.unwrap_or_default();
		
		// 创建新的文件名（游戏名称 + 扩展名）
		let new_filename = format!("{}{}", sanitize_filename(&game.name), extension);
		let new_path = parent_dir.join(&new_filename);
		
		// 重命名文件
		fs::rename(file_path, &new_path)
			.context(format!("无法将文件 '{}' 重命名为 '{}'", file_path.display(), new_path.display()))?;
		
		Ok(())
	}
}

// 清理文件名，移除非法字符
fn sanitize_filename(name: &str) -> String {
	name.chars()
		.map(|c| match c {
			'<' | '>' | ':' | '\"' | '/' | '\\' | '|' | '?' | '*' => '_',
			_ => c,
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

fn main() -> Result<(), Error> {
	let viewport = egui::ViewportBuilder::default()
		.with_inner_size([800.0, 600.0])  // 修改默认窗口大小
		.with_min_inner_size([600.0, 400.0])
		.with_title("retro-game-manager");
	
	// 尝试添加原生菜单项（如果egui支持）
	// 注意：这取决于egui版本，某些版本可能支持原生菜单
	
	let native_options = eframe::NativeOptions {
		viewport,
		..Default::default()
	};

	eframe::run_native(
		"retro-game-manager",  // 修改窗口标题为英文
		native_options,
		Box::new(|cc| {
			// 尝试设置原生菜单（如果API可用）
			// 这需要检查egui版本是否支持此功能
			Box::new(RetroGameManagerApp::new(cc).unwrap())
		}),
	)
}
