use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use eframe::egui;
use reqwest::blocking::Client;

// 图片加载结果
#[derive(Clone)]
pub enum ImageLoadResult {
    Loaded(egui::TextureHandle),
    NotFound,
    Loading,
}

// 图片加载器
pub struct ImageLoader {
    cache: Arc<Mutex<HashMap<String, ImageLoadResult>>>,
    client: Client,
}

impl ImageLoader {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            client: Client::new(),
        }
    }

    // 获取图片URL
    fn get_image_url(platform: &str, game_name: &str, image_type: &str) -> Option<String> {
        // 平台映射表：将平台名称映射到libretro-thumbnails的子模块名称
        let platform_map: HashMap<&str, &str> = [
            ("Nintendo - Game Boy", "Nintendo_-_Game_Boy"),
            ("Nintendo - Game Boy Advance", "Nintendo_-_Game_Boy_Advance"),
            ("Nintendo - Game Boy Color", "Nintendo_-_Game_Boy_Color"),
            ("Nintendo - Nintendo Entertainment System", "Nintendo_-_Nintendo_Entertainment_System"),
            ("Nintendo - Super Nintendo Entertainment System", "Nintendo_-_Super_Nintendo_Entertainment_System"),
            ("Nintendo - Nintendo 64", "Nintendo_-_Nintendo_64"),
            ("Nintendo - GameCube", "Nintendo_-_GameCube"),
            ("Nintendo - Wii", "Nintendo_-_Wii"),
            ("Nintendo - Wii U", "Nintendo_-_Wii_U"),
            ("Nintendo - Nintendo DS", "Nintendo_-_Nintendo_DS"),
            ("Nintendo - Nintendo 3DS", "Nintendo_-_Nintendo_3DS"),
            ("Sega - Mega Drive - Genesis", "Sega_-_Mega_Drive_-_Genesis"),
            ("Sega - Master System - Mark III", "Sega_-_Master_System_-_Mark_III"),
            ("Sega - Game Gear", "Sega_-_Game_Gear"),
            ("Sega - Saturn", "Sega_-_Saturn"),
            ("Sega - Dreamcast", "Sega_-_Dreamcast"),
            ("Sony - PlayStation", "Sony_-_PlayStation"),
            ("Sony - PlayStation 2", "Sony_-_PlayStation_2"),
            ("Sony - PlayStation 3", "Sony_-_PlayStation_3"),
            ("Sony - PlayStation Portable", "Sony_-_PlayStation_Portable"),
            ("Sony - PlayStation Vita", "Sony_-_PlayStation_Vita"),
            ("Atari - 2600", "Atari_-_2600"),
            ("Atari - 5200", "Atari_-_5200"),
            ("Atari - 7800", "Atari_-_7800"),
            ("Atari - Jaguar", "Atari_-_Jaguar"),
            ("Atari - Lynx", "Atari_-_Lynx"),
            ("NEC - TurboGrafx 16", "NEC_-_TurboGrafx_16"),
            ("NEC - PC Engine", "NEC_-_PC_Engine"),
            ("NEC - PC Engine CD", "NEC_-_PC_Engine_CD"),
            ("NEC - SuperGrafx", "NEC_-_SuperGrafx"),
            ("Bandai - WonderSwan", "Bandai_-_WonderSwan"),
            ("Bandai - WonderSwan Color", "Bandai_-_WonderSwan_Color"),
            ("SNK - Neo Geo Pocket", "SNK_-_Neo_Geo_Pocket"),
            ("SNK - Neo Geo Pocket Color", "SNK_-_Neo_Geo_Pocket_Color"),
            ("Microsoft - Xbox", "Microsoft_-_Xbox"),
            ("Microsoft - Xbox 360", "Microsoft_-_Xbox_360"),
            ("Commodore - Amiga", "Commodore_-_Amiga"),
            ("Commodore - 64", "Commodore_-_64"),
            ("Apple - II", "Apple_-_II"),
            ("Apple - IIGS", "Apple_-_IIGS"),
            ("3DO - Interactive Multiplayer", "3DO_-_Interactive_Multiplayer"),
            ("Amstrad - CPC", "Amstrad_-_CPC"),
            ("Coleco - ColecoVision", "Coleco_-_ColecoVision"),
            ("GCE - Vectrex", "GCE_-_Vectrex"),
            ("Magnavox - Odyssey2", "Magnavox_-_Odyssey2"),
            ("Mattel - Intellivision", "Mattel_-_Intellivision"),
            ("Microsoft - MSX", "Microsoft_-_MSX"),
            ("Microsoft - MSX2", "Microsoft_-_MSX2"),
            ("NEC - PC-88", "NEC_-_PC-88"),
            ("NEC - PC-98", "NEC_-_PC-98"),
            ("NEC - PC-FX", "NEC_-_PC-FX"),
            ("Nintendo - Family Computer Disk System", "Nintendo_-_Family_Computer_Disk_System"),
            ("Nintendo - Satellaview", "Nintendo_-_Satellaview"),
            ("Nintendo - Sufami Turbo", "Nintendo_-_Sufami_Turbo"),
            ("Nintendo - Virtual Boy", "Nintendo_-_Virtual_Boy"),
            ("Philips - CD-i", "Philips_-_CD-i"),
            ("SNK - Neo Geo CD", "SNK_-_Neo_Geo_CD"),
            ("SNK - Neo Geo", "SNK_-_Neo_Geo"),
            ("Watara - Supervision", "Watara_-_Supervision"),
        ].iter().cloned().collect();

        // 如果找到了对应的平台映射
        if let Some(thumb_platform) = platform_map.get(platform) {
            // 构造图片URL
            let url = format!(
                "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/{}/{}.png",
                thumb_platform,
                image_type,
                game_name.replace("/", "_").replace("\\\\", "_").replace(":", "_")
            );
            Some(url)
        } else {
            None
        }
    }

    // 异步加载图片
    pub fn load_image_async(
        &self,
        ctx: &egui::Context,
        platform: String,
        game_name: String,
        image_type: String,
    ) -> ImageLoadResult {
        let cache_key = format!("{}_{}_{}", platform, game_name, image_type);
        
        // 检查缓存
        {
            let cache = self.cache.lock().unwrap();
            if let Some(result) = cache.get(&cache_key) {
                return result.clone();
            }
        }
        
        // 标记为加载中
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(cache_key.clone(), ImageLoadResult::Loading);
        }
        
        // 获取图片URL
        let url = match Self::get_image_url(&platform, &game_name, &image_type) {
            Some(url) => url,
            None => {
                let mut cache = self.cache.lock().unwrap();
                cache.insert(cache_key, ImageLoadResult::NotFound);
                return ImageLoadResult::NotFound;
            }
        };
        
        // 克隆必要的数据
        let cache = Arc::clone(&self.cache);
        let ctx = ctx.clone();
        let client = self.client.clone();
        
        // 在后台线程中加载图片
        std::thread::spawn(move || {
            match client.get(&url).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        if let Ok(bytes) = response.bytes() {
                            // 尝试解码图片
                            if let Ok(img) = image::load_from_memory(&bytes) {
                                let rgba_image = img.to_rgba8();
                                let (width, height) = rgba_image.dimensions();
                                
                                // 创建egui纹理
                                let pixels: Vec<u8> = rgba_image.into_raw();
                                let image_buffer = egui::ColorImage::from_rgba_unmultiplied(
                                    [width as usize, height as usize],
                                    &pixels,
                                );
                                
                                let texture_handle = ctx.load_texture(
                                    format!("thumbnail_{}", cache_key),
                                    image_buffer,
                                    egui::TextureOptions::NEAREST,
                                );
                                
                                // 缓存纹理
                                let mut cache = cache.lock().unwrap();
                                cache.insert(cache_key, ImageLoadResult::Loaded(texture_handle));
                            } else {
                                // 图片解码失败
                                let mut cache = cache.lock().unwrap();
                                cache.insert(cache_key, ImageLoadResult::NotFound);
                            }
                        } else {
                            // 获取字节数据失败
                            let mut cache = cache.lock().unwrap();
                            cache.insert(cache_key, ImageLoadResult::NotFound);
                        }
                    } else {
                        // HTTP响应失败
                        let mut cache = cache.lock().unwrap();
                        cache.insert(cache_key, ImageLoadResult::NotFound);
                    }
                }
                Err(_) => {
                    // 网络请求失败
                    let mut cache = cache.lock().unwrap();
                    cache.insert(cache_key, ImageLoadResult::NotFound);
                }
            }
            
            // 请求重绘以更新UI
            ctx.request_repaint();
        });
        
        ImageLoadResult::Loading
    }
    
    // 批量加载三张图片
    pub fn load_game_images_async(
        &self,
        ctx: &egui::Context,
        platform: String,
        game_name: String,
    ) -> (ImageLoadResult, ImageLoadResult, ImageLoadResult) {
        let boxart = self.load_image_async(ctx, platform.clone(), game_name.clone(), "Named_Boxarts".to_string());
        let title = self.load_image_async(ctx, platform.clone(), game_name.clone(), "Named_Titles".to_string());
        let snap = self.load_image_async(ctx, platform, game_name, "Named_Snaps".to_string());
        (boxart, title, snap)
    }
}