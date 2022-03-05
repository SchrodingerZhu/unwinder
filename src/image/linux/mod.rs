use crate::image::builder;
use gimli::BaseAddresses;

pub struct Builder;

impl builder::Builder for Builder {
    fn mapper() -> builder::SectionMapper {
        vec![
            (".text", Box::new(BaseAddresses::set_text)),
            (".eh_frame", Box::new(BaseAddresses::set_eh_frame)),
            (".eh_frame_hdr", Box::new(BaseAddresses::set_eh_frame_hdr)),
            (".got", Box::new(BaseAddresses::set_got)),
        ]
    }
}
