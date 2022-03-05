use object::{File, Object, ObjectSection};

pub type SectionMapper = Vec<(
    &'static str,
    Box<dyn Fn(gimli::BaseAddresses, u64) -> gimli::BaseAddresses>,
)>;

pub trait Builder {
    fn mapper() -> SectionMapper;

    fn build(obj: &File) -> Option<gimli::BaseAddresses> {
        Self::mapper().iter().fold(
            Some(gimli::BaseAddresses::default()),
            |acc, (name, setter)| {
                acc.and_then(|a| obj.section_by_name(name).map(|s| setter(a, s.address())))
            },
        )
    }
}
