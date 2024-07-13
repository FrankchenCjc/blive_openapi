
use serde::Deserialize;

const API_PUB_BASE: &str = "https://live-open.biliapi.com";

const V2API_SUB_URL: &str = "/v2/app/";
const _API_MAP: [(&str, &str); 4] = [
    ("sta", "start"),
    ("end", "end"),
    ("hb", "heartbeat"),
    ("bhb", "batchHeartbeat"),
];

/// Data From start api
#[derive(Debug, Deserialize)]
pub struct ApiPkg {
    pub code: u16,
    pub message: String,
    pub data: ApiData,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ApiData {
    Start {
        game_info: GameInfo,
        websockt_info: WebsocktInfo,
        anchor_info: AnchorInfo,
    },
    BatchHeartbeat {
        failed_game_ids: Vec<String>,
    },
    Null,
}
#[derive(Debug, Deserialize)]
pub struct GameInfo {
    pub game_id: String,
}
#[derive(Debug, Deserialize)]
pub struct WebsocktInfo {
    pub auth_body: String,
    pub wss_link: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct AnchorInfo {
    pub room_id: u32,
    pub uname: String,
    pub uface: String,
    pub uid: u128,
    pub open_id: String,
}
