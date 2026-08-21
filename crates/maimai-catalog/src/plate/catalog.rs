use std::collections::HashMap;

use maimai_core::Music;

use crate::{
    CatalogError, TextNormalizer,
    metadata::CatalogMetadata,
    raw::{DxData, PlateDocument},
};

use super::{
    PlateMember, PlateName, PlateQuery, PlateServer,
    builtin::{build_cn, build_jp, cn_key, normalize_jp_name},
    custom::CustomPlateDocument,
    custom_catalog::build_custom,
    member::PlateMembers,
    source_index::SourceIndex,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct PlateCatalog {
    cn: HashMap<String, MemberSet>,
    jp: HashMap<String, MemberSet>,
    custom: HashMap<String, MemberSet>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct MemberSet {
    pub(super) declared: usize,
    pub(super) members: Vec<PlateMember>,
}

impl PlateCatalog {
    pub(crate) fn build(
        plate_document: PlateDocument,
        custom_document: CustomPlateDocument,
        dxdata: &DxData,
        songs: &[Music],
        metadata: &CatalogMetadata,
        normalizer: &TextNormalizer,
    ) -> Result<Self, CatalogError> {
        let sources = SourceIndex::build(songs, metadata, normalizer)?;
        let cn = build_cn(plate_document, &sources)?;
        let jp = build_jp(dxdata, &cn, &sources)?;
        let custom = build_custom(custom_document, &sources, normalizer)?;
        Ok(Self { cn, jp, custom })
    }

    pub(crate) fn membership(
        &self,
        query: &PlateQuery,
        normalizer: &TextNormalizer,
    ) -> super::PlateMembership {
        let values = self.get(query);
        let numeric_ids = values
            .members
            .iter()
            .filter_map(|member| member.identity().diving_fish_id())
            .collect();
        let title_generations = values
            .members
            .iter()
            .map(|member| (normalizer.normalize(member.title()), member.generation()))
            .collect();
        super::PlateMembership::new(
            query.clone(),
            values.declared,
            numeric_ids,
            title_generations,
            normalizer.clone(),
        )
    }

    pub(crate) fn members(&self, query: &PlateQuery) -> PlateMembers {
        let values = self.get(query);
        PlateMembers::new(query.clone(), values.declared, values.members.clone())
    }

    pub(crate) fn exists(&self, name: &PlateName, server: PlateServer) -> bool {
        let query = PlateQuery::new(name.clone(), server);
        !self.get(&query).members.is_empty()
    }

    fn get(&self, query: &PlateQuery) -> &MemberSet {
        let values = match query.server() {
            PlateServer::Cn => self.cn.get(cn_key(query.name().as_str())),
            PlateServer::Jp => {
                let key = normalize_jp_name(query.name().as_str());
                self.jp.get(&key)
            }
            PlateServer::Custom => self.custom.get(query.name().as_str()),
        };
        values.unwrap_or(&EMPTY)
    }
}

static EMPTY: MemberSet = MemberSet {
    declared: 0,
    members: Vec::new(),
};
