use crate::image::builder;
use gimli::BaseAddresses;

pub struct Builder;

impl builder::Builder for Builder {
    fn mapper() -> builder::SectionMapper {
        vec![
            ("__text", Box::new(BaseAddresses::set_text)),
            ("__eh_frame", Box::new(BaseAddresses::set_eh_frame)),
            ("__got", Box::new(BaseAddresses::set_got)),
        ]
    }
}
