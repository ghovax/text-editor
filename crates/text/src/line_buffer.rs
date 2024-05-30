use crate::{
    attributes::{Attributes, AttributesList},
    font_system::FontSystem,
    layouting::LayoutedLine,
    shaping::ShapedLine,
};

pub struct LineBuffer {
    pub text: String,
    pub attributes_list: AttributesList,
    pub shaped_line: Option<ShapedLine>,
    pub layouted_line: Option<LayoutedLine>,
}

impl LineBuffer {
    pub fn from_rich_text(content_spans: &[(String, Attributes)], default_attributes: Attributes) -> Self {
        let (reconstructed_string, mut spans_data): (String, Vec<_>) = {
            let mut end_index = 0;

            content_spans
                .iter()
                .map(|(span_text, attributes)| {
                    let start_index = end_index;
                    end_index += span_text.len();

                    (span_text.as_str(), (start_index..end_index, attributes.clone()))
                })
                .unzip()
        };

        let mut attributes_list = AttributesList::new(default_attributes);

        for (span_range, span_attributes) in spans_data.iter_mut() {
            let span_text = reconstructed_string.get(span_range.clone()).unwrap();

            if *span_attributes != attributes_list.default_attributes {
                attributes_list.add_span(span_range.clone(), *span_attributes)
            }
        }

        Self {
            attributes_list,
            text: reconstructed_string,
            shaped_line: None,
            layouted_line: None,
        }
    }

    pub fn invalidate_shaping(&mut self) {
        self.shaped_line = None;
    }

    pub fn invalidate_layout(&mut self) {
        self.shaped_line = None;
        self.layouted_line = None;
    }

    pub fn as_shaped_line(&mut self, font_system: &mut FontSystem) -> &ShapedLine {
        let shaped_line = self
            .shaped_line
            .get_or_insert_with(|| ShapedLine::new(font_system, &self.text, &self.attributes_list).unwrap());

        // Invalidate the layout of the line
        self.layouted_line = None;
        shaped_line
    }

    pub fn as_mut_layouted_line(&mut self, font_system: &mut FontSystem, font_size: f32) -> &mut LayoutedLine {
        if self.layouted_line.is_none() {
            let shaped_line = self.as_shaped_line(font_system);
            self.layouted_line = Some(shaped_line.layout(font_size));
        }

        self.layouted_line.as_mut().unwrap()
    }
}
