// 为了消除警告，暂时注释掉所有代码
// //! 从百度百科获取中文名称的模块

// use reqwest::blocking::Client;
// use scraper::{Html, Selector};
// use urlencoding::encode;
// use std::collections::HashMap;
// use std::sync::{Arc, Mutex};

// /// 一个简单的缓存，用于存储已经查询过的英文名和对应的中文名
// #[derive(Debug, Clone)]
// pub struct ChineseNameCache {
//     cache: Arc<Mutex<HashMap<String, Option<String>>>>,
// }

// impl ChineseNameCache {
//     /// 创建一个新的缓存实例
//     pub fn new() -> Self {
//         Self {
//             cache: Arc::new(Mutex::new(HashMap::new())),
//         }
//     }

//     /// 从缓存中获取中文名称
//     pub fn get(&self, english_name: &str) -> Option<Option<String>> {
//         self.cache.lock().unwrap().get(english_name).cloned()
//     }

//     /// 将中文名称存入缓存
//     pub fn insert(&self, english_name: String, chinese_name: Option<String>) {
//         self.cache.lock().unwrap().insert(english_name, chinese_name);
//     }
// }

// /// 从百度百科获取中文名称
// /// 
// /// # 参数
// /// * `english_name`: 要查询的英文名称
// /// * `cache`: 用于缓存查询结果的缓存实例
// /// 
// /// # 返回值
// /// 返回查询到的中文名称，如果查询失败或没有找到则返回 `None`
// pub fn get_chinese_name_from_baidu(english_name: &str, cache: &ChineseNameCache) -> Option<String> {
//     // 首先检查缓存
//     if let Some(cached) = cache.get(english_name) {
//         return cached;
//     }

//     // 创建HTTP客户端
//     let client = Client::builder()
//         .timeout(std::time::Duration::from_secs(10))
//         .build()
//         .ok()?;

//     // 构造百度百科搜索URL
//     let encoded_name = encode(english_name);
//     let search_url = format!("https://baike.baidu.com/search?word={}&pn=0&rn=1&srt=0", encoded_name);

//     // 发送GET请求
//     match client.get(&search_url).send() {
//         Ok(response) => {
//             if response.status().is_success() {
//                 // 解析HTML响应
//                 if let Ok(html) = response.text() {
//                     let document = Html::parse_document(&html);

//                     // 查找第一个搜索结果的链接
//                     let search_result_selector = Selector::parse("div.search-list a").unwrap();
//                     if let Some(first_result) = document.select(&search_result_selector).next() {
//                         if let Some(href) = first_result.value().attr("href") {
//                             // 如果是相对链接，构造完整URL
//                             let full_url = if href.starts_with("/") {
//                                 format!("https://baike.baidu.com{}", href)
//                             } else {
//                                 href.to_string()
//                             };

//                             // 访问词条页面
//                             if let Ok(entry_response) = client.get(&full_url).send() {
//                                 if entry_response.status().is_success() {
//                                     if let Ok(entry_html) = entry_response.text() {
//                                         let entry_document = Html::parse_document(&entry_html);

//                                         // 查找词条标题（通常是中文名称）
//                                         let title_selector = Selector::parse("h1.title-text").unwrap();
//                                         if let Some(title_element) = entry_document.select(&title_selector).next() {
//                                             let title = title_element.text().collect::<Vec<_>>().join("");
//                                             if !title.is_empty() {
//                                                 // 缓存结果
//                                                 cache.insert(english_name.to_string(), Some(title.clone()));
//                                                 return Some(title);
//                                             }
//                                         }

//                                         // 如果找不到h1.title-text，尝试查找其他可能的标题元素
//                                         let alt_title_selector = Selector::parse("h1.lemma-title").unwrap();
//                                         if let Some(title_element) = entry_document.select(&alt_title_selector).next() {
//                                             let title = title_element.text().collect::<Vec<_>>().join("");
//                                             if !title.is_empty() {
//                                                 // 缓存结果
//                                                 cache.insert(english_name.to_string(), Some(title.clone()));
//                                                 return Some(title);
//                                             }
//                                         }

//                                         // 如果还找不到，尝试查找页面标题
//                                         let page_title_selector = Selector::parse("title").unwrap();
//                                         if let Some(title_element) = entry_document.select(&page_title_selector).next() {
//                                             let title = title_element.text().collect::<Vec<_>>().join("");
//                                             // 移除"百度百科"后缀
//                                             let clean_title = title.split("_").next().unwrap_or(&title).to_string();
//                                             if !clean_title.is_empty() && !clean_title.contains("百度百科") {
//                                                 // 缓存结果
//                                                 cache.insert(english_name.to_string(), Some(clean_title.clone()));
//                                                 return Some(clean_title);
//                                             }
//                                         }
//                                     }
//                                 }
//                             }
//                         }
//                     }
//                 }
//             }
//         }
//         Err(_) => {
//             // 网络请求失败，返回None
//         }
//     }

//     // 如果所有方法都失败，缓存None并返回None
//     cache.insert(english_name.to_string(), None);
//     None
// }