use gimli::BaseAddresses;
use object::{File, Object, ObjectSection};

pub type SectionMapper = &'static [(
    &'static str,
    fn(gimli::BaseAddresses, u64) -> gimli::BaseAddresses,
)];

const BASE_MAPPERS: SectionMapper = &[
    (".text", BaseAddresses::set_text),
    (".eh_frame", BaseAddresses::set_eh_frame),
    (".got", BaseAddresses::set_got),
];
const OPTIONAL_MAPPERS: SectionMapper = &[(".eh_frame_hdr", BaseAddresses::set_eh_frame_hdr)];

pub fn build(obj: &File) -> Option<gimli::BaseAddresses> {
    let ba = BASE_MAPPERS.iter().fold(
        Some(gimli::BaseAddresses::default()),
        |acc, (name, setter)| {
            acc.and_then(|a| obj.section_by_name(name).map(|s| setter(a, s.address())))
        },
    );
    OPTIONAL_MAPPERS.iter().fold(ba, |acc, (name, setter)| {
        if let Some(sec) = obj.section_by_name(name) {
            return acc.map(|a| setter(a, sec.address()));
        }
        acc
    })
}
