//! The app's icons, one typed name each. macOS draws them as SF Symbols
//! (`assets.rs`); the Lucide files in `assets/icons/` serve other platforms.

macro_rules! icons {
    ($($name:ident $symbol:literal $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon { $($name,)* }

        impl Icon {
            #[cfg(test)]
            const ALL: &[Icon] = &[$(Icon::$name,)*];

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
    Tag "tag" "tag",
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
    Hide "eye.slash" "eye",
    Highlight "highlighter" "pen-line",
    Home "house" "home",
    HomeFilled "house.fill" "home-filled",
    Import "square.and.arrow.down" "import",
    Info "info.circle" "info",
    Library "books.vertical" "library",
    Minus "minus" "minus",
    Move "folder" "folder-input",
    Notes "note.text" "doc",
    Palette "paintpalette" "palette",
    Paper "doc.richtext" "doc",
    Pin "pin" "pin",
    Plus "plus" "plus",
    Quote "text.quote" "quote",
    Restore "arrow.uturn.backward" "restore",
    Search "magnifyingglass" "search",
    Select "checklist" "batch",
    Show "eye" "eye",
    PanelToggle "sidebar.right" "panel-left-close",
    SidebarToggle "sidebar.left" "panel-left-close",
    Details "sidebar.right" "panel-right",
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

#[cfg(test)]
mod tests {
    use super::Icon;

    #[test]
    fn every_icon_has_its_file() {
        for icon in Icon::ALL {
            let file = format!("{}/assets/icons/{}.svg", env!("CARGO_MANIFEST_DIR"), icon.file());
            assert!(std::path::Path::new(&file).is_file(), "{icon:?} names a missing {file}");
        }
    }
}
