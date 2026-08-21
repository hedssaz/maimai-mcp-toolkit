use super::CatalogSourceClient;
use super::decode::{DecodedDocument, envelope_list_len, invalid_shape};
use super::error::{CatalogSourceError, EndpointId};
use super::request::RequestOptions;
use super::types::{CatalogSource, DocumentStatistics, SourceBundle, SourceMetadata, SourceTarget};

pub(crate) async fn fetch(
    client: &CatalogSourceClient,
) -> Result<SourceBundle, CatalogSourceError> {
    let songs = client
        .request(
            EndpointId::LxnsSongs,
            RequestOptions {
                lxns_notes: true,
                ..RequestOptions::default()
            },
        )
        .await?
        .require_updated(CatalogSource::Lxns, EndpointId::LxnsSongs)?;
    let aliases = client
        .request(EndpointId::LxnsAliases, RequestOptions::default())
        .await?
        .require_updated(CatalogSource::Lxns, EndpointId::LxnsAliases)?;

    let songs = DecodedDocument::parse(songs, CatalogSource::Lxns, EndpointId::LxnsSongs)?;
    let song_count = envelope_list_len(songs.value(), &["content", "songs", "aliases"])
        .ok_or_else(|| invalid_shape(CatalogSource::Lxns, EndpointId::LxnsSongs))?;
    let aliases = DecodedDocument::parse(aliases, CatalogSource::Lxns, EndpointId::LxnsAliases)?;
    let alias_count = envelope_list_len(aliases.value(), &["content", "songs", "aliases"])
        .ok_or_else(|| invalid_shape(CatalogSource::Lxns, EndpointId::LxnsAliases))?;

    let documents = vec![
        songs.into_document(
            SourceTarget::LxnsSongList,
            DocumentStatistics::Records {
                records: song_count,
            },
        ),
        aliases.into_document(
            SourceTarget::LxnsAliasList,
            DocumentStatistics::Records {
                records: alias_count,
            },
        ),
    ];
    Ok(SourceBundle::updated(
        CatalogSource::Lxns,
        documents,
        None,
        SourceMetadata::empty(),
    ))
}
