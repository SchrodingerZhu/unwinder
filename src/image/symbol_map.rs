use object::{File, Object, SymbolMap, SymbolMapEntry, SymbolMapName};

#[derive(Debug)]
pub struct OwnedSymbolMapName {
    address: u64,
    name: String,
}

impl OwnedSymbolMapName {
    pub fn new<S: AsRef<str>>(address: u64, name: S) -> Self {
        OwnedSymbolMapName {
            address,
            name: name.as_ref().to_string(),
        }
    }

    /// The symbol address.
    #[inline]
    pub fn address(&self) -> u64 {
        self.address
    }

    /// The symbol name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn from(origin: &SymbolMapName) -> Self {
        Self::new(origin.address(), origin.name())
    }
}

impl SymbolMapEntry for OwnedSymbolMapName {
    #[inline]
    fn address(&self) -> u64 {
        self.address
    }
}

pub type OwnedSymbolMap = SymbolMap<OwnedSymbolMapName>;

pub fn load(f: &File) -> OwnedSymbolMap {
    SymbolMap::new(
        f.symbol_map()
            .symbols()
            .into_iter()
            .map(OwnedSymbolMapName::from)
            .collect(),
    )
}
