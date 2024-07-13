use bytes::{Buf, BufMut, BytesMut};
use futures_util::{
    future::ok,
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::Deserialize;
use std::usize;
use tokio::{net::TcpStream, sync::broadcast};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{protocol, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::error;

use super::{ApiError, Terror};

#[derive(Debug)]
pub struct WsLink {
    open_id: String,
    rx: tokio::sync::Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
    tx: tokio::sync::Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    stop_mark: broadcast::Receiver<bool>,
}
impl WsLink {
    pub async fn new(
        open_id: String,
        verfive: String,
        addr: String,
        stop_mark: broadcast::Receiver<bool>,
    ) -> Result<Self, Terror> {
        let (tx, rx) = connect_async(addr).await?.0.split();
        let mut this_link = Self {
            open_id,
            tx: tokio::sync::Mutex::new(tx),
            rx: tokio::sync::Mutex::new(rx),
            stop_mark,
        };
        this_link.verfive(verfive).await?;
        Ok(this_link)
    }
    async fn verfive(&mut self, verfive: String) -> Result<(), Terror> {
        let verfive: BytesMut = (&mut Proto::new_verifive(verfive)).try_into()?;
        let verfive = verfive.to_vec();
        let mut tx = self.tx.lock().await;
        tx.send(Message::Binary(verfive)).await?;
        drop(tx);
        let mut rx = self.rx.lock().await;
        let _: Proto = match rx.next().await {
            Some(Ok(Message::Binary(msg))) => {
                let msg = &mut BytesMut::from(msg.as_slice());
                msg.try_into()?
            }
            Some(Ok(t)) => {
                error!("get some unexpect data{}", t);
                return Err(Box::new(ApiError::new(11)));
            }
            Some(Err(e)) => {
                error!("{}", e);
                return Err(Box::new(e));
            }
            None => {
                error!("None get wile recive verfive send back",);
                return Err(Box::new(ApiError::new(12)));
            }
        };
        Ok(())
    }
    fn create_heartbeatjobs(self) -> Result<(), Terror> {
        tokio::spawn(async move {
            let mut clock = tokio::time::interval(tokio::time::Duration::from_secs(25));
            loop {
                if !self.stop_mark.is_empty() {
                    if self.stop_mark.recv().await.or::<bool>(Ok(true)) == Ok(true) {
                        break;
                    }
                };
                clock.tick().await;
                let hb: BytesMut = match (&mut Proto::new_heat_beat()).try_into() {
                    Ok(d) => d,
                    Err(e) => {
                        error!("heartbeat sending job while stop, error case : {}", e);
                        break;
                    }
                };
                let hb = hb.to_vec();
                let mut tx = self.tx.lock().await;
                tx.send(Message::Binary(hb)).await;
                drop(tx);
            }
            let mut tx = self.tx.lock().await;
            tx.close().await;
        });
        Ok(())
    }
}

#[derive(Debug)]
pub struct Header {
    ///total size of proto
    totalsize: u32,
    ///header size of this struct normaly is 16
    headersize: u16,
    ///version if proto if 2,is zlib proto
    version: u16,
    /// opearation code
    /// - 2: heartbeat
    /// - 3: heartbeat responde
    /// - 5: normal proto
    /// - 7: verivfive proto
    /// - 8: verivfive responde
    opecode: u32,
    ///proto sequence
    seq: u32,
}

#[derive(Debug)]
pub enum Body {
    /// for blank or useless data
    Blank,
    ///to descrecpt recving someting
    Pkg(Box<RecData>),
    ///if sending back more then one proto by zlib
    ProtoVec(Vec<Proto>),
    ///someting to Send
    Send(Box<BytesMut>),
}
impl Body {
    fn try_into_bytesmut(&mut self) -> Result<&BytesMut, Terror> {
        Ok(match self {
            Body::Send(str) => str,
            _ => {
                tracing::error!("bytes unsended message");
                return Err(Box::new(super::ApiError::new(0)));
            }
        })
    }
    fn try_from_bytesmut(value: &mut BytesMut, is_compress_packages: bool) -> Result<Body, Terror> {
        match is_compress_packages {
            true => {
                let mut tmp = Vec::new();
                while value.has_remaining() {
                    let tosize = value.get_u32();
                    let mut tmpbytes = BytesMut::new();
                    tmpbytes.put_u32(tosize);
                    let tosize = tosize as usize;
                    tmpbytes.put_slice(match value.get(0..tosize - 4) {
                        Some(d) => d,
                        None => {
                            error!("no data readin ");
                            return Err(Box::new(ApiError::new(0)));
                        }
                    });
                    let tmpproto: Proto = (&mut tmpbytes).try_into()?;
                    tmp.push(tmpproto);
                    value.advance(tosize);
                }
                Ok(Body::ProtoVec(tmp))
            }
            false => {
                let recdata: RecData =
                    serde_json::from_slice(match value.get(0..value.remaining()) {
                        Some(d) => d,
                        None => {
                            error!("no data readin ");
                            return Err(Box::new(ApiError::new(0)));
                        }
                    })?;
                Ok(Body::Pkg(Box::new(recdata)))
            }
        }
    }
}

#[derive(Debug)]
pub struct Proto {
    ///header of proto
    header: Header,
    ///proto body
    body: Body,
}
impl Proto {
    fn new_heat_beat() -> Proto {
        Proto {
            header: Header {
                totalsize: 16 + 15,
                headersize: 16,
                version: 0,
                opecode: 2,
                seq: 0,
            },
            body: Body::Send(Box::new(BytesMut::from("FROM_BILI_LIGHT".as_bytes()))),
        }
    }
    fn new_verifive(verfive_token: String) -> Proto {
        let verfive_bytes = BytesMut::from(verfive_token.as_bytes());
        Proto {
            header: Header {
                totalsize: verfive_bytes.len() as u32 + 16,
                headersize: 16,
                version: 0,
                opecode: 7,
                seq: 0,
            },
            body: Body::Send(Box::new(verfive_bytes)),
        }
    }
}
impl TryFrom<&mut Proto> for BytesMut {
    type Error = Terror;
    fn try_from(value: &mut Proto) -> Result<Self, Self::Error> {
        let mut buf = BytesMut::new();
        buf.put_u32(value.header.totalsize);
        buf.put_u16(value.header.headersize);
        buf.put_u16(value.header.version);
        buf.put_u32(value.header.opecode);
        buf.put_u32(value.header.seq);
        buf.put_slice(value.body.try_into_bytesmut()?);
        Ok(buf)
    }
}

impl TryFrom<&mut BytesMut> for Proto {
    type Error = Terror;
    fn try_from(value: &mut BytesMut) -> Result<Self, Self::Error> {
        let totalsize = value.get_u32();
        let headersize = value.get_u16();
        let version = value.get_u16();
        let opecode = value.get_u32();
        let seq = value.get_u32();
        let header = Header {
            totalsize,
            headersize,
            version,
            opecode,
            seq,
        };
        let body: Body = match opecode {
            5 => Body::try_from_bytesmut(
                &mut BytesMut::from(match value.get(0..value.remaining()) {
                    Some(d) => d,
                    None => {
                        error!("no data readin ");
                        return Err(Box::new(ApiError::new(0)));
                    }
                }),
                version == 2,
            )?,
            3 => Body::Blank,
            8 => {
                if BytesMut::from(match value.get(0..value.remaining()) {
                    Some(d) => d,
                    None => {
                        error!("no data readin ");
                        return Err(Box::new(ApiError::new(0)));
                    }
                })
                .to_vec()
                .as_slice()
                    == b"{\"code\" = 0}"
                {
                    Body::Blank
                } else {
                    error!("verfive error ");
                    return Err(Box::new(ApiError::new(10)));
                }
            }
            _ => {
                error!("unkonw proto status or sending type");
                return Err(Box::new(ApiError::new(10)));
            }
        };
        Ok(Proto { header, body })
    }
}

impl Proto {
    pub fn new(header: Header, body: Body) -> Self {
        Self { header, body }
    }
}

#[derive(Debug, Deserialize)]
pub struct RecData {
    ///comment proto cmdline
    cmd: String,
    ///implent data
    data: WspData,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum WspData {
    Dm {
        ///用户昵称
        uname: String,
        ///用户UID（已废弃，固定为0）
        uid: i64,
        ///用户唯一标识
        open_id: String,
        ///用户头像
        uface: String,
        ///时间秒级
        timestamp: i64,
        ///直播间id
        room_id: i64,
        ///留言内容
        msg: String,
        ///消息唯一id
        msg_id: String,
        ///对应房间大航海等级
        guard_level: i64,
        ///该房间粉丝勋章佩戴情况
        fans_medal_wearing_status: bool,
        ///对应房间勋章名字
        fans_medal_name: String,
        ///对应房间勋章信息
        fans_medal_level: i64,
        emoji_img_url: String,
        dm_type: i64,
    },
    Sc {
        ///直播间id
        room_id: i64,
        ///用户UID（已废弃，固定为0）
        uid: i64,
        ///用户唯一标识
        open_id: String,
        ///购买的用户昵称
        uname: String,
        ///购买用户头像
        uface: String,
        ///留言id(风控场景下撤回留言需要)
        message_id: i64,
        ///留言内容
        message: String,
        ///支付金额(元)
        rmb: i64,
        ///赠送时间秒级
        timestamp: i64,
        ///生效开始时间
        start_time: i64,
        ///生效结束时间
        end_time: i64,
        ///对应房间大航海等级
        guard_level: i64,
        ///对应房间勋章信息
        fans_medal_level: i64,
        ///对应房间勋章名字
        fans_medal_name: String,
        ///该房间粉丝勋章佩戴情况
        fans_medal_wearing_status: bool,
        ///消息唯一id
        msg_id: String,
    },
    Like {
        ///用户昵称
        uname: String,
        ///用户UID（已废弃，固定为0）
        uid: i64,
        ///用户唯一标识
        open_id: String,
        ///用户头像
        uface: String,
        ///时间秒级时间戳
        timestamp: i64,
        ///发生的直播间
        room_id: i64,
        ///点赞文案( “xxx点赞了”)
        like_text: String,
        ///对单个用户最近2秒的点赞次数聚合
        like_count: i64,
        ///该房间粉丝勋章佩戴情况
        fans_medal_wearing_status: bool,
        ///粉丝勋章名
        fans_medal_name: String,
        ///对应房间勋章信息
        fans_medal_level: i64,
    },
    Gift {
        ///房间号
        room_id: i64,
        ///用户UID（已废弃，固定为0）
        uid: i64,
        ///用户唯一标识
        open_id: String,
        ///送礼用户昵称
        uname: String,
        ///送礼用户头像
        uface: String,
        ///道具id(盲盒:爆出道具id)
        gift_id: i64,
        ///道具名(盲盒:爆出道具名)
        gift_name: String,
        ///赠送道具数量
        gift_num: i64,
        ///礼物爆出单价，(1000 = 1元 = 10电池),盲盒:爆出道具的价值
        price: i64,
        ///是否是付费道具
        paid: bool,
        ///实际送礼人的勋章信息
        fans_medal_level: i64,
        ///粉丝勋章名
        fans_medal_name: String,
        ///该房间粉丝勋章佩戴情况
        fans_medal_wearing_status: bool,
        ///大航海等级
        guard_level: i64,
        ///收礼时间秒级时间戳
        timestamp: i64,
        ///主播信息
        anchor_info: GiftAnchorInfo,
        ///消息唯一id
        msg_id: String,
        ///道具icon
        gift_icon: String,
        ///是否是combo道具
        combo_gift: bool,
        ///连击信息
        combo_info: GiftComboInfo,
    },
}
#[derive(Debug, Deserialize)]
pub struct GiftAnchorInfo {
    pub uname: String,
    pub uface: String,
    pub uid: u128,
    pub open_id: String,
}
#[derive(Debug, Deserialize)]
pub struct GiftComboInfo {
    ///每次连击赠送的道具数量
    pub combo_base_num: i64,
    ///连击次数
    pub combo_count: i64,
    ///连击id
    pub combo_id: String,
    ///连击有效期秒
    pub combo_timeout: i64,
}
