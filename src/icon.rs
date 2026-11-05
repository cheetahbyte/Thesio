//! The app's icons, one typed name each. macOS draws them as SF Symbols
//! (`assets.rs`); the Lucide files in `assets/icons/` serve other platforms.

macro_rules! icons {
    ($($name:ident $symbol:literal $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon { $($name,)* }

        impl Icon {
            pub fn symbol(self) -> &'static str {
                match self { $(Icon::$name => $symbol,)* }
            }

            fn file(self) -> &'static str {
                match self { $(Icon::$name => $file,)* }
            }
        }
    };
}

icons! {
    Back "chevron.left" "arrow-left",
    Check "checkmark" "check",
    ChevronDown "chevron.down" "chevron-down",
    ChevronRight "chevron.right" "chevron-right",
    ChevronUp "chevron.up" "chevron-up",
    Circle "circle.fill" "circle-filled",
    Clear "xmark.circle.fill" "search-clear",
    Close "xmark" "x",
    Copy "doc.on.doc" "copy",
    Delete "trash" "bin",
    Document "doc.text" "doc",
    Edit "pencil" "square-pen",
    Ellipsis "ellipsis" "ellipsis",
    Export "square.and.arrow.up" "export",
    External "arrow.up.right.square" "export",
    Folder "folder" "folder",
    FolderNew "folder.badge.plus" "folder-plus",
    Gear "gearshape" "settings-gear",
    Grid "square.grid.2x2" "view-grid",
    Hide "eye.slash" "eye",
    Home "house" "home",
    HomeFilled "house.fill" "home-filled",
    Import "square.and.arrow.down" "import",
    Info "info.circle" "info",
    Keyboard "keyboard" "keyboard",
    Library "books.vertical" "library",
    List "list.bullet" "view-list",
    Minus "minus" "minus",
    Move "folder" "folder-input",
    Palette "paintpalette" "palette",
    Paper "doc.richtext" "doc",
    Pin "pin" "pin",
    PinFilled "pin.fill" "pin-filled",
    Plus "plus" "plus",
    Quote "text.quote" "quote",
    Restore "arrow.uturn.backward" "restore",
    Search "magnifyingglass" "search",
    Select "checklist" "batch",
    Show "eye" "eye",
    SidebarToggle "sidebar.left" "panel-left-close",
    Sort "arrow.up.arrow.down" "sort-down",
    Star "star" "star",
    Storage "internaldrive" "storage",
    Unpin "pin.slash" "pin-off",
    Unstar "star.slash" "star-off",
    Window "macwindow" "copy",
    Maximize "arrow.up.left.and.arrow.down.right" "expand",
}

impl Icon {
    /// What `svg().path` loads: `sf/<symbol>` on macOS, a bundled file elsewhere.
    /// Small glyphs take a heavier cut, as AppKit's own disclosure chevrons do.
    pub fn path(self, size: f32) -> String {
        if !cfg!(target_os = "macos") { return format!("icons/{}.svg", self.file()); }
        format!("sf/{}{}", self.symbol(), if size <= 12. { "/medium" } else { "" })
    }
}
