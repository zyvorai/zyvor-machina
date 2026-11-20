use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::types::array_compatible;
use crate::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue as JsonRawValue;
use serde_json::Value as JsonValue;
pub(crate) use sqlx_core::types::{Json, Type};

// <https://www.postgresql.org/docs/12/datatype-json.html>

// In general, most applications should prefer to store JSON data as jsonb,
// unless there are quite specialized needs, such as legacy assumptions
// about ordering of object keys.

impl<T> Type<Postgres> for Json<T> {
    // machina: JSON documents are TEXT columns in the controller's schema, written compact exactly as SQLite stores them (JSONB
    // would reformat the text, and a TEXT column rejects a JSONB parameter on some paths)
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::TEXT
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        // machina: JSON documents stored as TEXT (SQLite has no JSON type) decode too
        *ty == PgTypeInfo::JSON || *ty == PgTypeInfo::JSONB || *ty == PgTypeInfo::TEXT
    }
}

impl<T> PgHasArrayType for Json<T> {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::JSONB_ARRAY
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        array_compatible::<Json<T>>(ty)
    }
}

impl PgHasArrayType for JsonValue {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::JSONB_ARRAY
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        array_compatible::<JsonValue>(ty)
    }
}

impl PgHasArrayType for JsonRawValue {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::JSONB_ARRAY
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        array_compatible::<JsonRawValue>(ty)
    }
}

impl<'q, T> Encode<'q, Postgres> for Json<T>
where
    T: Serialize,
{
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        // machina: plain compact JSON text (the parameter type is TEXT, see `type_info` above); real JSON/JSONB parameters are not
        // used by the controller

        // the JSON data written to the buffer is the same regardless of parameter type
        serde_json::to_writer(&mut **buf, &self.0)?;

        Ok(IsNull::No)
    }
}

impl<'r, T: 'r> Decode<'r, Postgres> for Json<T>
where
    T: Deserialize<'r>,
{
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let mut buf = value.as_bytes()?;

        if value.format() == PgValueFormat::Binary && value.type_info == PgTypeInfo::JSONB {
            assert_eq!(
                buf[0], 1,
                "unsupported JSONB format version {}; please open an issue",
                buf[0]
            );

            buf = &buf[1..];
        }

        serde_json::from_slice(buf).map(Json).map_err(Into::into)
    }
}
