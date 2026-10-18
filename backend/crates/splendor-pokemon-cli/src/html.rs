//! 自包含 HTML 回放：模板内嵌 CSS / JS，回放 JSON 直接写进 `<script>`。
//!
//! 生成结果不引用任何外部资源，`file://` 打开即可播放。

use splendor_pokemon_core::Replay;

const TEMPLATE: &str = include_str!("../assets/replay.html");

pub(crate) fn render(replay: &Replay) -> Result<String, String> {
    let json = serde_json::to_string(replay).map_err(|error| format!("回放序列化失败: {error}"))?;
    // 防止卡牌名称或日志中的 `</script>` 提前闭合数据块；`<\/` 仍是合法 JSON。
    let json = json.replace("</", "<\\/");
    Ok(TEMPLATE.replace("__REPLAY_JSON__", &json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use splendor_pokemon_core::{play_bot_game, BotKind};

    #[test]
    fn rendered_html_embeds_parseable_replay_and_no_external_assets() {
        let replay = play_bot_game(
            vec!["A".into(), "B".into()],
            3,
            &[BotKind::Heuristic, BotKind::Random],
        )
        .expect("game should finish");
        let html = render(&replay).expect("render html");

        assert!(!html.contains("__REPLAY_JSON__"));
        for forbidden in ["http://", "https://", "cdn.", "<link"] {
            assert!(!html.contains(forbidden), "external reference: {forbidden}");
        }
        for id in [
            "board", "supply", "players", "action", "banner", "slider", "play", "prev", "next",
        ] {
            assert!(
                html.contains(&format!("id=\"{id}\"")),
                "missing element #{id}"
            );
        }

        let marker = r#"<script id="replay-data" type="application/json">"#;
        let start = html.find(marker).expect("data block") + marker.len();
        let end = start + html[start..].find("</script>").expect("data block end");
        let loaded = Replay::from_json(&html[start..end]).expect("embedded json");
        assert_eq!(loaded.seed, replay.seed);
        assert_eq!(loaded.frames.len(), replay.frames.len());
    }
}
