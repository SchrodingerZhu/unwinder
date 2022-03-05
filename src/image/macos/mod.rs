use crate::image::builder;
use gimli::BaseAddresses;

pub struct Builder;

impl builder::Builder for Builder {
    fn mapper() -> builder::SectionMapper {
        vec![
            ("__text", BaseAddresses::set_text),
            ("__eh_frame", BaseAddresses::set_eh_frame),
            ("__got", BaseAddresses::set_got),
        ]
    }
}
