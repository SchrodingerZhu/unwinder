use gimli::BaseAddresses;
use object::{File, Object, ObjectSection};

pub type SectionMapper = &'static [(
    &'static str,
    fn(gimli::BaseAddresses, u64) -> gimli::BaseAddresses,
)];

const BASE_SEC_MAPPERS: SectionMapper = &[
    (".text", BaseAddresses::set_text),
    (".eh_frame", BaseAddresses::set_eh_frame),
    (".got", BaseAddresses::set_got),
];
const EXTRA_SEC_MAPPERS: SectionMapper = &[(".eh_frame_hdr", BaseAddresses::set_eh_frame_hdr)];

pub fn load(f: &File) -> Option<gimli::BaseAddresses> {
    let ba = BASE_SEC_MAPPERS.iter().fold(
        Some(gimli::BaseAddresses::default()),
        |acc, (name, setter)| {
            acc.and_then(|a| f.section_by_name(name).map(|s| setter(a, s.address())))
        },
    );
    EXTRA_SEC_MAPPERS.iter().fold(ba, |acc, (name, setter)| {
        if let Some(sec) = f.section_by_name(name) {
            return acc.map(|a| setter(a, sec.address()));
        }
        acc
    })
}
