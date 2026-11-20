use byteorder::{BigEndian, ByteOrder};

use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::types::Type;
use crate::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres};

impl Type<Postgres> for f32 {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::FLOAT4
    }

    // machina: accept any PostgreSQL number (SQLite's REAL has one width, and AVG() is NUMERIC here)
    fn compatible(ty: &PgTypeInfo) -> bool {
        super::lenient::is_float(ty) || super::lenient::is_int(ty) || *ty == PgTypeInfo::NUMERIC
    }
}

impl PgHasArrayType for f32 {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::FLOAT4_ARRAY
    }
}

impl Encode<'_, Postgres> for f32 {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        buf.extend(&self.to_be_bytes());

        Ok(IsNull::No)
    }
}

impl Decode<'_, Postgres> for f32 {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        // machina: read whatever number type the column is
        if value.format() == PgValueFormat::Binary {
            let ty = value.type_info.clone();
            if ty == PgTypeInfo::FLOAT8 {
                return Ok(BigEndian::read_f64(value.as_bytes()?) as f32);
            } else if ty == PgTypeInfo::NUMERIC {
                return Ok(super::lenient::numeric_to_f64(value.as_bytes()?)? as f32);
            } else if super::lenient::is_int(&ty) {
                return Ok(<i64 as Decode<Postgres>>::decode(value)? as f32);
            }
        }
        Ok(match value.format() {
            PgValueFormat::Binary => BigEndian::read_f32(value.as_bytes()?),
            PgValueFormat::Text => value.as_str()?.parse()?,
        })
    }
}

impl Type<Postgres> for f64 {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::FLOAT8
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        super::lenient::is_float(ty) || super::lenient::is_int(ty) || *ty == PgTypeInfo::NUMERIC
    }
}

impl PgHasArrayType for f64 {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::FLOAT8_ARRAY
    }
}

impl Encode<'_, Postgres> for f64 {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        buf.extend(&self.to_be_bytes());

        Ok(IsNull::No)
    }
}

impl Decode<'_, Postgres> for f64 {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        // machina: read whatever number type the column is
        if value.format() == PgValueFormat::Binary {
            let ty = value.type_info.clone();
            if ty == PgTypeInfo::FLOAT4 {
                return Ok(BigEndian::read_f32(value.as_bytes()?) as f64);
            } else if ty == PgTypeInfo::NUMERIC {
                return Ok(super::lenient::numeric_to_f64(value.as_bytes()?)?);
            } else if super::lenient::is_int(&ty) {
                return Ok(<i64 as Decode<Postgres>>::decode(value)? as f64);
            }
        }
        Ok(match value.format() {
            PgValueFormat::Binary => BigEndian::read_f64(value.as_bytes()?),
            PgValueFormat::Text => value.as_str()?.parse()?,
        })
    }
}
