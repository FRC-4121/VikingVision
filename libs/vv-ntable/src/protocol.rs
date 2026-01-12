use serde::Serialize;

/// An type that's serialized as an empty map.
///
/// I don't want to actually support properties, but we serialize an empty map anyways.
pub struct EmptyMap;
impl Serialize for EmptyMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(std::iter::empty::<((), ())>())
    }
}

// #[derive(Serialize, Deserialize)]
// pub struct Properties {
//     persistent: bool,
//     retained: bool,
//     cached: bool,
// }

// #[derive(Deserialize)]
// #[serde(tag = "method", content = "params", rename_all = "lowercase")]
// pub enum ServerToClientMessage {
//     Announce {
//         name: String,
//         id: u32,
//         r#type: String,
//         pubuid: Option<u32>,
//         properties: Properties,
//     },
//     #[serde(other)]
//     Unknown,
// }

/// Messages we send to the server
///
/// Right now, it's just publish.
#[derive(Serialize)]
#[serde(tag = "method", content = "params", rename_all = "lowercase")]
pub enum ClientToServerMessage<'a> {
    Publish {
        name: &'a str,
        pubuid: u32,
        r#type: &'static str,
        properties: EmptyMap,
    },
    // Unpublish {
    //     pubuid: u32,
    // },
    // SetProperties {
    //     pubuid: u32,
    //     properties: Properties,
    // },
}
