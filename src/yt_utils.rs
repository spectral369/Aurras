use lazy_static_include::lazy_static::lazy_static;
use linked_hash_set::LinkedHashSet;
use regex::Regex;
use std::borrow::Cow;

use std::ops::{Bound, RangeBounds};

trait StringUtils {
    fn substring(&self, start: usize, len: usize) -> &str;
    fn slice(&self, range: impl RangeBounds<usize>) -> &str;
}
// https://users.rust-lang.org/t/how-to-get-a-substring-of-a-string/1351/11 solution
impl StringUtils for str {
    fn substring(&self, start: usize, len: usize) -> &str {
        let mut char_pos = 0;
        let mut byte_start = 0;
        let mut it = self.chars();
        loop {
            if char_pos == start {
                break;
            }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_start += c.len_utf8();
            } else {
                break;
            }
        }
        char_pos = 0;
        let mut byte_end = byte_start;
        loop {
            if char_pos == len {
                break;
            }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_end += c.len_utf8();
            } else {
                break;
            }
        }
        &self[byte_start..byte_end]
    }
    fn slice(&self, range: impl RangeBounds<usize>) -> &str {
        let start = match range.start_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => 0,
        };
        let len = match range.end_bound() {
            Bound::Included(bound) => *bound + 1,
            Bound::Excluded(bound) => *bound,
            Bound::Unbounded => self.len(),
        } - start;
        self.substring(start, len)
    }
}
#[derive(Debug, Default, Clone)]
pub struct YtInfo {
    _yt_link: String,
    yt_desc: String,
    _yt_title: String,
    _yt_length_sec: String,
    _yt_thumbnail: String,
    _yt_author: String,
    _is_live: bool,
}

impl YtInfo {
    pub fn _set_yt_link(&mut self, value: String) {
        self._yt_link = value;
    }
    pub fn _set_yt_desc(&mut self, value: String) {
        self.yt_desc = value;
    }
    pub fn _get_yt_link(&self) -> String {
        self._yt_link.clone()
    }
    pub fn get_yt_desc(&self) -> String {
        self.yt_desc.clone()
    }
}
/* same as: #[derive(Default)]
impl Default for YtInfo {
    fn default() -> Self {
        YtInfo {
            _yt_link: String::default(),
            yt_desc: String::default(),
            yt_title: String::default(),
            yt_length_sec: String::default(),
            yt_thumbnail: String::default(),
            yt_author: String::default(),
            is_live: bool::default(),
        }
    }
}
*/
pub fn _extract_links(content: &str) -> LinkedHashSet<Cow<str>> {
    //pub fn extract_links(content: &str) -> YtInfo {
    // let mut fileRef = std::fs::File::create("saved.txt").expect("create failed");
    //std::io::Write::write_all(&mut fileRef, &content.as_bytes()).expect("write failed");
    let init_content_first_index = content.find("ytConfigData"); //orig ytInitialData

    let pre_unparsed_content = content
        .substring(init_content_first_index.unwrap(), content.len())
        .to_string();

    let init_content_last_index = pre_unparsed_content.find("</script>");

    let unparsed_content = pre_unparsed_content
        .substring(0, init_content_last_index.unwrap())
        .to_string();

    lazy_static! {
        static ref YT_LINK_REGEX: Regex = Regex::new("\\{\"videoId\":\"(.*?)\"").unwrap();
    }

    let mut links: LinkedHashSet<_> = YT_LINK_REGEX
        .captures_iter(&unparsed_content.to_string())
        .take(3)
        .map(|c| match c.get(1) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();
    links.reserve(links.len());

    links
}

pub fn get_link_content(content: &str, yt_link: String) -> YtInfo {
    // let mut fileRef = std::fs::File::create("saved.txt").expect("create failed");
    // std::io::Write::write_all(&mut fileRef, &content.as_bytes()).expect("write failed");
    lazy_static! {
        static ref YT_DESC_REGEX: Regex =
            Regex::new("(shortDescription\":\"(.*?)\"([^\"]*)\")").unwrap();
        static ref YT_TITLE_REGEX: Regex = Regex::new("(title\":\"(.*?)\"([^\"]*)\")").unwrap();
        static ref YT_LENGTH_SEC: Regex =
            Regex::new("(lengthSeconds\":\"(.*?)\"([^\"]*)\")").unwrap();
        static ref YT_THUMBNAIL: Regex =
            Regex::new("(thumbnails\":\\[\\{\"url\":\"(.*?)\"([^\"]*)\")").unwrap();
        static ref YT_AUTHOR: Regex = Regex::new("(author\":\"(.*?)\"([^\"]*)\")").unwrap();
        static ref YT_IS_LIVE: Regex = Regex::new("(isLiveContent\":+(\\w+))").unwrap();
    }

    /*for cap in YT_THUMBNAIL.captures(&content) {
        println!("2: {} 3: {} 1: {} ", &cap[2], &cap[3], &cap[1]);
    }*/

    let yt_desc: String = YT_DESC_REGEX
        .captures_iter(&content.to_string())
        .take(1)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();

    //   println!("{:?}", yt_desc);

    let yt_title: String = YT_TITLE_REGEX
        .captures_iter(&content.to_string())
        .take(1)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();

    println!(" -> {:?}", yt_title);

    let yt_len_sec: String = YT_LENGTH_SEC
        .captures_iter(&content.to_string())
        .take(1)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();

    println!("{:?}", yt_len_sec);

    let mut yt_thumbnail: String = YT_THUMBNAIL
        .captures_iter(&content.to_string())
        .take(1)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();
    let index_element = yt_thumbnail
        .chars()
        .position(|x| x == '?')
        .unwrap_or(yt_thumbnail.chars().count());
    yt_thumbnail = yt_thumbnail.chars().take(index_element).collect();
    println!("{:?}", yt_thumbnail);

    let yt_author: String = YT_AUTHOR
        .captures_iter(&content.to_string())
        .take(1)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();

    println!("{:?}", yt_author);
    let yt_is_live: String = YT_IS_LIVE
        .captures_iter(&content.to_string())
        .take(3)
        .map(|c| match c.get(2) {
            Some(val) => Cow::from(val.as_str().to_string()),
            _ => unreachable!(),
        })
        .collect();

    let yt_info_con = YtInfo {
        _yt_link: yt_link,
        yt_desc: String::from(yt_desc),
        _yt_title: String::from(yt_title),
        _yt_length_sec: String::from(yt_len_sec),
        _yt_thumbnail: String::from(yt_thumbnail),
        _yt_author: String::from(yt_author),
        _is_live: yt_is_live.parse().unwrap(),
    };
    yt_info_con
}
