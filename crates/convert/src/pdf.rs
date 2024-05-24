use fontdb::Source;
use pdf_writer::types::{
    ActionType, AnnotationType, BorderType, CidFontType, FontFlags, SystemInfo, UnicodeCmap,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use siphasher::sip128::{Hasher128, SipHasher13};
use std::collections::BTreeMap;
use std::fs;
use std::hash::Hash;
use ttf_parser::GlyphId;

const SYSTEM_INFO: SystemInfo = SystemInfo {
    registry: Str(b"Adobe"),
    ordering: Str(b"Identity"),
    supplement: 0,
};
const CMAP_NAME: Name = Name(b"Custom");
