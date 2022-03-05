use crate::image::builder;
use gimli::BaseAddresses;

pub struct Builder;

impl builder::Builder for Builder {
    fn mapper() -> builder::SectionMapper {
        vec![
            (".text", BaseAddresses::set_text),
            (".eh_frame", BaseAddresses::set_eh_frame),
            (".eh_frame_hdr", BaseAddresses::set_eh_frame_hdr),
            (".got", BaseAddresses::set_got),
        ]
    }
}
