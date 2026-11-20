use uuid::Uuid;

use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::types::Type;
use crate::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres};

impl Type<Postgres> for Uuid {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::UUID
    }

    // machina: an id kept as text decodes too
    fn compatible(ty: &PgTypeInfo) -> bool {
        *ty == PgTypeInfo::UUID || *ty == PgTypeInfo::TEXT
    }
}

impl PgHasArrayType for Uuid {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::UUID_ARRAY
    }
}

impl Encode<'_, Postgres> for Uuid {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(self.as_bytes());

        Ok(IsNull::No)
    }
}

impl Decode<'_, Postgres> for Uuid {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        if let Some(text) = super::lenient::text_of(&value) {
            return text?.parse().map_err(Into::into);
        }
        match value.format() {
            PgValueFormat::Binary => Uuid::from_slice(value.as_bytes()?),
            PgValueFormat::Text => value.as_str()?.parse(),
        }
        .map_err(Into::into)
    }
}
