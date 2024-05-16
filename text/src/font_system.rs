use std::{collections::HashMap, sync::Arc};

use crate::attributes::Attributes;

pub struct Font {
    swash_cache_key: (u32, swash::CacheKey),
    face: rustybuzz::Face<'static>,
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    id: fontdb::ID,
    unicode_codepoints: Vec<u32>,
}

impl Font {
    pub fn new(database: &fontdb::Database, font_id: fontdb::ID) -> Option<Self> {
        let face_info = database.face(font_id)?;

        let unicode_codepoints = {
            database.with_face_data(font_id, |font_data, face_index| {
                let face = rustybuzz::ttf_parser::Face::parse(font_data, face_index).ok()?;
                let mut unicode_codepoints = Vec::new();

                face.tables()
                    .cmap?
                    .subtables
                    .into_iter()
                    .filter(|subtable| subtable.is_unicode())
                    .for_each(|subtable| {
                        unicode_codepoints.reserve(1024);
                        subtable.codepoints(|code_point| {
                            if subtable.glyph_index(code_point).is_some() {
                                unicode_codepoints.push(code_point);
                            }
                        });
                    });

                unicode_codepoints.shrink_to_fit();

                Some(unicode_codepoints)
            })?
        }?;

        let data = match &face_info.source {
            fontdb::Source::Binary(data) => Arc::clone(data),
            fontdb::Source::File(path) => {
                log::warn!("Unsupported `fontdb::Source::File({:?})`", path.display());
                return None;
            }
            fontdb::Source::SharedFile(_path, data) => Arc::clone(data),
        };

        Some(Self {
            id: face_info.id,
            unicode_codepoints,
            swash_cache_key: {
                let swash = swash::FontRef::from_index((*data).as_ref(), face_info.index as usize)?;
                (swash.offset, swash.key)
            },
            face: rustybuzz::Face::try_new(Arc::clone(&data), |data| {
                rustybuzz::Face::from_slice((**data).as_ref(), face_info.index).ok_or(())
            })
            .ok()?,
            data,
        })
    }
}

/// Font-specific part of [`Attributes`] to be used for matching.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FontMatchAttributes {
    family: fontdb::Family<'static>,
    stretch: fontdb::Stretch,
    style: fontdb::Style,
    weight: fontdb::Weight,
}

impl<'a> From<Attributes> for FontMatchAttributes {
    fn from(attributes: Attributes) -> Self {
        let Attributes {
            family,
            stretch,
            style,
            weight,
            ..
        } = attributes;

        Self {
            family,
            stretch,
            style,
            weight,
        }
    }
}

pub struct FontMatchKey {
    pub font_weight_difference: u16,
    pub font_weight: u16,
    pub id: fontdb::ID,
}

/// Access to the system fonts.
pub struct FontSystem {
    /// The locale of the system.
    locale: String,
    /// The underlying font database.
    database: fontdb::Database,
    /// Cache for loaded fonts from the database.
    font_cache: HashMap<fontdb::ID, Option<Arc<Font>>>,
    /// Cache for font matches.
    font_matches_cache: HashMap<FontMatchAttributes, Arc<Vec<FontMatchKey>>>,
    /// Cache for `rustybuzz` shape plans.
    shape_plan_cache: ShapePlanCache,
}

impl FontSystem {
    /// Creates a new font system.
    pub fn new() -> Self {
        let locale = sys_locale::get_locale().unwrap_or_else(|| {
            log::warn!("Failed to get system locale, falling back to en-US");
            String::from("en-US")
        });

        let mut database = fontdb::Database::new();
        database.set_default_language(locale.as_str());
        database.set_default_family(fontdb::Family::Serif);

        // TODO(ghovax): The user might want to load additional fonts.
        database.set_monospace_family("CMU Typewriter Text");
        database.set_sans_serif_family("CMU Sans Serif");
        database.set_serif_family("CMU Serif");

        database.load_system_fonts();
        log::debug!("Parsed {} font faces", database.len(),);

        Self {
            locale,
            database,
            font_cache: HashMap::new(),
            font_matches_cache: HashMap::new(),
            shape_plan_cache: ShapePlanCache::new(),
        }
    }

    /// Get a font from the database by its ID.
    pub fn get_font(&mut self, font_id: fontdb::ID) -> Option<Arc<Font>> {
        self.font_cache
            .entry(font_id)
            .or_insert_with(|| {
                unsafe {
                    self.database.make_shared_face_data(font_id);
                }
                match Font::new(&self.database, font_id) {
                    Some(font) => Some(Arc::new(font)),
                    None => {
                        log::warn!(
                            "Failed to load the font {:?} from the database",
                            self.database.face(font_id)?.post_script_name
                        );
                        None
                    }
                }
            })
            .clone()
    }
}
