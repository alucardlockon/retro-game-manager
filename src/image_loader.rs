use eframe::egui;
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::Path; // 添加 Path 导入
use walkdir::WalkDir; // 添加 walkdir 导入
use std::ffi::OsStr; // 添加 OsStr 导入

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
    // 动态平台映射表
    platform_map: Arc<Mutex<HashMap<String, String>>>,
}

impl ImageLoader {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            client: Client::new(),
            platform_map: Arc::new(Mutex::new(HashMap::new())), // 初始化 platform_map
        }
    }

    // 新增：初始化 platform_map 的方法
    pub fn initialize_platform_map(&self, xmldb_path: &Path) {
        let mut map = self.platform_map.lock().unwrap();
        map.clear(); // 清空现有映射

        // 扫描 xmldb 目录
        for entry in WalkDir::new(xmldb_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension() == Some(OsStr::new("xml")))
        {
            let path = entry.path();
            if let Some(file_stem) = path.file_stem() {
                let platform_name_raw = file_stem.to_string_lossy();
                // 从文件名推断平台名称 (与 xml.rs 中的逻辑一致)
                let platform_name = platform_name_raw
                    .split(" (")
                    .next()
                    .unwrap_or(&platform_name_raw)
                    .to_string();
                
                // 尝试将平台名称转换为 libretro-thumbnails 的格式
                // 这里是一个简化的转换规则，你可能需要根据实际情况调整
                let thumb_platform_name = platform_name
                    .replace(" - ", "_-_")
                    .replace(" ", "_")
                    .replace("/", "_")
                    .replace(":", "_");

                // 插入映射 (如果尚未存在，避免覆盖)
                map.entry(platform_name).or_insert(thumb_platform_name);
            }
        }
        
        // 你可以在这里添加一些已知的、需要特殊处理的映射
        // 例如，如果文件名推断不准确，可以手动覆盖
        // map.insert("Some Special Platform".to_string(), "Some_Special_Platform".to_string());
        
        println!("Initialized platform map with {} entries.", map.len());
    }


    // 获取图片URL
    fn get_image_url(&self, platform: &str, game_name: &str, image_type: &str) -> Option<String> {
        // 锁定并获取映射表的引用
        let map = self.platform_map.lock().unwrap();
        
        // 如果找到了对应的平台映射
        if let Some(thumb_platform) = map.get(platform) {
            // 构造图片URL
            let url = format!(
                "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/{}/{}.png",
                thumb_platform,
                image_type,
                game_name
                    .replace("/", "_")
                    .replace("\\", "_")
                    .replace(":", "_")
            );
            Some(url)
        } else {
            // 如果没有找到映射，可以选择返回 None 或者尝试一个默认的猜测
            // 这里我们选择返回 None
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

        // 获取图片URL (使用实例方法)
        let url = match self.get_image_url(&platform, &game_name, &image_type) {
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
        let boxart = self.load_image_async(
            ctx,
            platform.clone(),
            game_name.clone(),
            "Named_Boxarts".to_string(),
        );
        let title = self.load_image_async(
            ctx,
            platform.clone(),
            game_name.clone(),
            "Named_Titles".to_string(),
        );
        let snap = self.load_image_async(ctx, platform, game_name, "Named_Snaps".to_string());
        (boxart, title, snap)
    }
}
