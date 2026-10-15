//! 検索元(エンジン)の定義と、公開の検索結果ページ(HTML)の解析。
//!
//! 検索元ごとの違いは、コードではなく**設定(`EngineDef`)**にする。URL の型と、結果の
//! 「かたまり・タイトル・リンク・要約」を指す CSS セレクタだけである。検索元がページの作りを
//! 変えても、セレクタの設定を直せば追従できる(`maintain` が AI の提案を検証して自動で直す)。

use anyhow::{anyhow, bail, Result};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EngineDef {
    pub id: String,
    pub name: String,
    /// 検索 URL の型。`{q}`(検索語)・`{hl}`(言語)・`{gl}`(地域)・`{kl}`(DuckDuckGo 形式)を置き換える
    pub url: String,
    /// 1件の結果のかたまり
    pub container: String,
    pub title: String,
    pub link: String,
    pub snippet: String,
    /// リンクの URL を読む属性。空なら link 要素の `href`。`data-href` のように別の属性も、`@mu` のように
    /// 結果のかたまり(container)自身の属性も指定できる
    #[serde(default)]
    pub link_attr: String,
    /// この検索元への呼び出しの最小の間隔(ミリ秒)。0 なら全体の既定(1.2秒)。拒否されやすい検索元は長くする
    #[serde(default)]
    pub interval_ms: u64,
    /// 既定の設定の版。コードで既定の設定を直したとき(例: 結果のかたまりの指定を直した)に上げる。
    /// 保存済みの設定の版が既定より古ければ、既定に置き換える(AI が直した新しい設定は、版が同じなら残す)
    #[serde(default)]
    pub rev: u32,
    /// 転送 URL(例: DuckDuckGo の `uddg`)から本来の URL を取り出すためのパラメータ名。無ければ空
    #[serde(default)]
    pub unwrap: String,
    /// 統合するときの重み
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// この検索元が得意な言語(基本コード。例: ["ja"])。空なら全言語向け
    #[serde(default)]
    pub langs: Vec<String>,
    /// true なら、`langs` の言語の検索でしか使わない(例: 日本向けの検索元)
    #[serde(default)]
    pub only_langs: bool,
    /// AI が「セレクタでは直せない(give_up)」と連続で判断した回数。`maintain::DISABLE_AFTER_GIVE_UPS`
    /// に達すると自動で無効化する(`maintain::selfcheck`)。直せた・正常だった時点で 0 に戻る。
    #[serde(default)]
    pub give_up_streak: u32,
    /// 検索元の種類。空(または `html`)なら、公開の検索結果ページ(HTML)をセレクタで読む。
    /// `wikipedia`・`hn`・`stackoverflow` は、**プログラムからの利用が公式に認められた無料の公開 API**
    /// (JSON)を読む(セレクタ・自動修復の対象外)。HTML の検索結果ページが `robots.txt` で禁止されている・
    /// ボット検知される英語圏の検索元の代わりに、英語の検索を安定させるために使う
    #[serde(default)]
    pub kind: String,
}

/// 公式の公開 API として扱う種類
const API_KINDS: [&str; 3] = ["wikipedia", "hn", "stackoverflow"];

fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Hit {
    pub title: String,
    pub link: String,
    pub snippet: String,
}

pub fn defaults() -> Vec<EngineDef> {
    serde_json::from_str(include_str!("../engines.default.json"))
        .expect("既定の検索元の設定は正しい")
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl EngineDef {
    /// 検索 URL を作る。`hl` は言語(例: ja)、`gl` は地域(例: JP)。
    pub fn search_url(&self, q: &str, hl: &str, gl: &str) -> String {
        let hl = if hl.is_empty() { "en" } else { hl };
        let base = hl.split('-').next().unwrap_or(hl);
        let kl = if gl.is_empty() {
            "wt-wt".to_string() // DuckDuckGo の「地域を指定しない」
        } else {
            format!("{}-{}", gl.to_ascii_lowercase(), base.to_ascii_lowercase())
        };
        let url = self
            .url
            .replace("{q}", &encode(q))
            .replace("{hl}", &encode(hl))
            .replace("{lang}", &encode(&base.to_ascii_lowercase()))
            .replace("{gl}", &encode(gl))
            .replace("{kl}", &encode(&kl));
        // 地域が分からないときは、地域の指定そのものを付けない
        url.replace("&cc=&", "&")
            .trim_end_matches("&cc=")
            .to_string()
    }

    /// 公式の公開 API(JSON)を読む検索元か。
    pub fn is_api(&self) -> bool {
        API_KINDS.contains(&self.kind.as_str())
    }

    /// この検索元を、この言語の検索に使うか。
    pub fn serves(&self, lang_code: &str) -> bool {
        let base = lang_code.split('-').next().unwrap_or(lang_code);
        !self.only_langs || self.langs.iter().any(|l| l == base || l == lang_code)
    }

    /// この言語を得意とする検索元か(結果の統合で重くする)
    pub fn favours(&self, lang_code: &str) -> bool {
        let base = lang_code.split('-').next().unwrap_or(lang_code);
        self.langs.iter().any(|l| l == base || l == lang_code)
    }

    /// 設定が安全で使えるか(AI が提案した設定を取り込む前にも使う)。
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("id が正しくありません");
        }
        let u = reqwest::Url::parse(&self.search_url("test", "en", "US"))
            .map_err(|e| anyhow!("URL の型が正しくありません: {e}"))?;
        if u.scheme() != "https" {
            bail!("URL は https のみです");
        }
        if !self.kind.is_empty() && self.kind != "html" && !self.is_api() {
            bail!("kind が正しくありません");
        }
        if self.is_api() {
            // API の検索元は、セレクタを使わない
            if !(0.0..=2.0).contains(&self.weight) {
                bail!("weight は 0〜2 です");
            }
            return Ok(());
        }
        for (name, sel) in [
            ("container", &self.container),
            ("title", &self.title),
            ("link", &self.link),
            ("snippet", &self.snippet),
        ] {
            if sel.len() > 300 {
                bail!("{name} のセレクタが長すぎます");
            }
            if sel.is_empty() && name == "snippet" {
                continue; // 要約が無い検索元もある
            }
            Selector::parse(sel)
                .map_err(|e| anyhow!("{name} のセレクタが正しくありません: {e:?}"))?;
        }
        if self.link_attr.len() > 40
            || !self
                .link_attr
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '@'))
        {
            bail!("link_attr が正しくありません");
        }
        if !(0.0..=2.0).contains(&self.weight) {
            bail!("weight は 0〜2 です");
        }
        Ok(())
    }
}

/// `//duckduckgo.com/l/?uddg=<URL>` のような転送 URL から、本来の URL を取り出す。
/// Bing の転送 URL(`bing.com/ck/a?...&u=a1<base64url>`)から本来の URL を取り出す。
fn decode_bing(href: &str) -> Option<String> {
    let u = reqwest::Url::parse(href).ok()?;
    if !u.host_str()?.ends_with("bing.com") {
        return None;
    }
    let (_, v) = u.query_pairs().find(|(k, _)| k == "u")?;
    let b64 = v.strip_prefix("a1")?;
    let mut bits: u32 = 0;
    let mut nbits = 0;
    let mut out = Vec::new();
    for c in b64.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '-' | '+' => 62,
            '_' | '/' => 63,
            '=' => break,
            _ => return None,
        };
        bits = (bits << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push(((bits >> nbits) & 0xFF) as u8);
        }
    }
    let s = String::from_utf8(out).ok()?;
    (s.starts_with("http://") || s.starts_with("https://")).then_some(s)
}

fn unwrap_redirect(href: &str, param: &str) -> String {
    if let Some(real) = decode_bing(href) {
        return real;
    }
    if param.is_empty() {
        return href.to_string();
    }
    let full = if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        href.to_string()
    };
    if let Ok(u) = reqwest::Url::parse(&full) {
        if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == param) {
            return v.into_owned();
        }
    }
    href.to_string()
}

fn text_of(e: scraper::ElementRef<'_>) -> String {
    e.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// HTML のタグを取り除き、基本的な文字参照を戻した文字列にする(API の要約・題名用)。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn json_str<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// 公式の公開 API(JSON)の応答から、検索結果を取り出す。形が想定と違えば空(=「読み取れず」)にする。
fn parse_api(kind: &str, body: &str) -> Vec<Hit> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut out: Vec<Hit> = Vec::new();
    match kind {
        // MediaWiki API(generator=search + prop=info|extracts)。pages は pageid をキーにした連想配列で、
        // 並びは各ページの index(検索の順位)で決める
        "wikipedia" => {
            let Some(pages) = v.pointer("/query/pages").and_then(|p| p.as_object()) else {
                return out;
            };
            let mut items: Vec<(&serde_json::Value, i64)> = pages
                .values()
                .map(|p| (p, p.get("index").and_then(|i| i.as_i64()).unwrap_or(i64::MAX)))
                .collect();
            items.sort_by_key(|(_, i)| *i);
            for (p, _) in items {
                let (title, link) = (json_str(p, "title"), json_str(p, "fullurl"));
                if title.is_empty() || !link.starts_with("https://") {
                    continue;
                }
                out.push(Hit {
                    title: title.to_string(),
                    link: link.to_string(),
                    snippet: strip_html(json_str(p, "extract")),
                });
            }
        }
        // Hacker News の Algolia API
        "hn" => {
            for h in v.get("hits").and_then(|h| h.as_array()).into_iter().flatten() {
                let title = json_str(h, "title");
                let id = json_str(h, "objectID");
                if title.is_empty() || id.is_empty() {
                    continue;
                }
                let link = match json_str(h, "url") {
                    u if u.starts_with("https://") || u.starts_with("http://") => u.to_string(),
                    _ => format!("https://news.ycombinator.com/item?id={id}"),
                };
                let points = h.get("points").and_then(|x| x.as_i64()).unwrap_or(0);
                let comments = h.get("num_comments").and_then(|x| x.as_i64()).unwrap_or(0);
                let date: String = json_str(h, "created_at").chars().take(10).collect();
                out.push(Hit {
                    title: strip_html(title),
                    link,
                    snippet: format!("Hacker News · {points} points · {comments} comments · {date}"),
                });
            }
        }
        // Stack Exchange API の /search/excerpts(Stack Overflow)
        "stackoverflow" => {
            for it in v.get("items").and_then(|h| h.as_array()).into_iter().flatten() {
                let title = strip_html(json_str(it, "title"));
                let Some(qid) = it.get("question_id").and_then(|x| x.as_i64()) else {
                    continue;
                };
                if title.is_empty() {
                    continue;
                }
                out.push(Hit {
                    title,
                    link: format!("https://stackoverflow.com/questions/{qid}"),
                    snippet: strip_html(json_str(it, "excerpt")),
                });
            }
        }
        _ => {}
    }
    out
}

/// 結果ページの HTML から、検索結果を取り出す。広告(検索元の宣伝リンク)は除く。
pub fn parse(def: &EngineDef, html: &str) -> Vec<Hit> {
    if def.is_api() {
        return parse_api(&def.kind, html);
    }
    let snip = Selector::parse(&def.snippet).ok();
    let (Ok(cont), Ok(title), Ok(link)) = (
        Selector::parse(&def.container),
        Selector::parse(&def.title),
        Selector::parse(&def.link),
    ) else {
        return Vec::new();
    };
    let doc = Html::parse_document(html);
    let mut out: Vec<Hit> = Vec::new();
    for c in doc.select(&cont) {
        let Some(t) = c
            .select(&title)
            .next()
            .map(text_of)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let href = match def.link_attr.strip_prefix('@') {
            Some(name) => c.value().attr(name),
            None => {
                let attr = if def.link_attr.is_empty() {
                    "href"
                } else {
                    def.link_attr.as_str()
                };
                c.select(&link).next().and_then(|a| a.value().attr(attr))
            }
        };
        let Some(href) = href else {
            continue;
        };
        let url = unwrap_redirect(href, &def.unwrap);
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            continue;
        }
        // 検索元自身の広告・転送は結果にしない
        if url.contains("duckduckgo.com/y.js") || url.contains("bing.com/aclick") {
            continue;
        }
        let s = snip
            .as_ref()
            .and_then(|sel| c.select(sel).next())
            .map(text_of)
            .unwrap_or_default();
        if out.iter().any(|h| h.link == url) {
            continue;
        }
        out.push(Hit {
            title: t,
            link: url,
            snippet: s,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_def(id: &str) -> EngineDef {
        defaults().into_iter().find(|e| e.id == id).unwrap()
    }

    #[test]
    fn api_engines_are_valid_and_use_the_query_language_subdomain() {
        for id in ["wikipedia", "hackernews", "stackoverflow"] {
            let d = api_def(id);
            assert!(d.is_api(), "{id}");
            d.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        let u = api_def("wikipedia").search_url("Rust 言語", "ja", "JP");
        assert!(u.starts_with("https://ja.wikipedia.org/w/api.php?"), "{u}");
        assert!(u.contains("gsrsearch=Rust+%E8%A8%80%E8%AA%9E"), "{u}");
        let tw = api_def("wikipedia").search_url("x", "zh-TW", "TW");
        assert!(tw.starts_with("https://zh.wikipedia.org/"), "{tw}");
    }

    #[test]
    fn wikipedia_api_results_keep_search_rank_order() {
        let body = r#"{"query":{"pages":{"2":{"title":"B","index":2,"fullurl":"https://en.wikipedia.org/wiki/B","extract":"about <b>b</b>"},"1":{"title":"A","index":1,"fullurl":"https://en.wikipedia.org/wiki/A","extract":"about a"}}}}"#;
        let hits = parse(&api_def("wikipedia"), body);
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(hits[1].snippet, "about b");
    }

    #[test]
    fn hacker_news_and_stack_overflow_results_are_read() {
        let hn = r#"{"hits":[{"title":"Rust 1.99 released","url":"https://blog.rust-lang.org/x","objectID":"1","points":300,"num_comments":42,"created_at":"2026-10-02T10:00:00Z"},{"title":"Ask HN: Rust?","url":null,"objectID":"2","points":5,"num_comments":1,"created_at":"2026-10-03T10:00:00Z"}]}"#;
        let h = parse(&api_def("hackernews"), hn);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].link, "https://blog.rust-lang.org/x");
        assert_eq!(h[1].link, "https://news.ycombinator.com/item?id=2");
        assert!(h[0].snippet.contains("300 points") && h[0].snippet.contains("2026-10-02"));
        let so = r#"{"items":[{"item_type":"question","question_id":123,"title":"How to use &quot;Rust&quot; traits?","excerpt":"use <span class=\"highlight\">traits</span> like this"}]}"#;
        let s = parse(&api_def("stackoverflow"), so);
        assert_eq!(s[0].link, "https://stackoverflow.com/questions/123");
        assert_eq!(s[0].title, "How to use \"Rust\" traits?");
        assert_eq!(s[0].snippet, "use traits like this");
    }

    #[test]
    fn malformed_api_bodies_yield_no_hits() {
        for id in ["wikipedia", "hackernews", "stackoverflow"] {
            assert!(parse(&api_def(id), "<html>blocked</html>").is_empty());
            assert!(parse(&api_def(id), "{}").is_empty());
        }
    }

    fn ddg() -> EngineDef {
        defaults()
            .into_iter()
            .find(|e| e.id == "duckduckgo")
            .unwrap()
    }

    #[test]
    fn defaults_are_valid_and_urls_are_encoded() {
        for e in defaults() {
            e.validate().unwrap_or_else(|err| panic!("{}: {err}", e.id));
        }
        let u = ddg().search_url("山梨県 温泉", "ja", "JP");
        assert!(
            u.contains("q=%E5%B1%B1%E6%A2%A8%E7%9C%8C+%E6%B8%A9%E6%B3%89")
                && u.contains("kl=jp-ja"),
            "{u}"
        );
    }

    #[test]
    fn parses_excite_style_results() {
        let html = r#"<html><body>
          <div class="item">
            <h3 class="item_title"><a href="http://www.seotonoyu.jp/">【公式】瀬音の湯</a></h3>
            <p class="item_summary">ご宿泊された翌日は一般営業開始前の午前9時45分より<b>温泉</b>をご利用頂けます。</p>
            <div class="item_status"><span class="item_name">www.seotonoyu.jp</span></div>
          </div>
          <div class="item">
            <h3 class="item_title"><a href="https://onsen.nifty.com/">あきる野市の日帰り温泉</a></h3>
            <p class="item_summary">お得な割引クーポンや宿泊プランを多数掲載。</p>
          </div>
        </body></html>"#;
        let excite = defaults().into_iter().find(|e| e.id == "excite").unwrap();
        let hits = parse(&excite, html);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].link, "http://www.seotonoyu.jp/");
        assert_eq!(hits[0].title, "【公式】瀬音の湯");
        assert!(hits[0].snippet.contains("温泉"));
    }

    #[test]
    fn parses_duckduckgo_style_results_and_skips_ads() {
        let html = r#"<html><body>
          <div class="result results_links web-result"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&amp;rut=x">Example A</a></h2><a class="result__snippet">About A</a></div>
          <div class="result result--ad"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fad.example%2F">AD</a></h2></div>
          <div class="result"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fduckduckgo.com%2Fy.js%3Fad_domain%3Dx">Sneaky ad</a></h2></div>
          <div class="result"><h2><a class="result__a" href="javascript:alert(1)">Bad</a></h2></div>
          <div class="result"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa">Dup</a></h2></div>
          <div class="result"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fb">Example B</a></h2></div>
        </body></html>"#;
        let hits = parse(&ddg(), html);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(
            hits[0],
            Hit {
                title: "Example A".into(),
                link: "https://example.com/a".into(),
                snippet: "About A".into()
            }
        );
        assert_eq!(hits[1].link, "https://example.org/b");
    }

    #[test]
    fn decodes_bing_redirect_links() {
        // https://example.com/a?b=1 を base64url にして a1 を付けたもの
        let href = "https://www.bing.com/ck/a?!&&p=x&u=a1aHR0cHM6Ly9leGFtcGxlLmNvbS9hP2I9MQ&ntb=1";
        assert_eq!(unwrap_redirect(href, ""), "https://example.com/a?b=1");
        let other = "https://other.example/?u=a1aHR0cHM6Ly9leGFtcGxlLmNvbS9hP2I9MQ";
        assert_eq!(unwrap_redirect(other, ""), other);
    }

    #[test]
    fn empty_region_omits_cc_and_language_gating_works() {
        let bing = defaults().into_iter().find(|e| e.id == "bing").unwrap();
        let u = bing.search_url("x", "th", "");
        assert!(!u.contains("cc="), "{u}");
        let y = defaults().into_iter().find(|e| e.id == "yahoo-jp").unwrap();
        assert!(y.serves("ja") && !y.serves("en") && !y.serves("zh-Hans"));
        assert!(y.favours("ja") && !bing.favours("ja") && bing.serves("ko"));
    }

    #[test]
    fn validate_rejects_unsafe_or_broken_definitions() {
        let mut e = ddg();
        e.url = "http://insecure.example/?q={q}".into();
        assert!(e.validate().is_err());
        let mut e = ddg();
        e.container = "div[".into();
        assert!(e.validate().is_err());
        let mut e = ddg();
        e.id = "../x".into();
        assert!(e.validate().is_err());
    }
}
