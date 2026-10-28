# dmgbuild settings for the installer window. render.sh builds a throwaway
# DMG with these only to take its .DS_Store; the release DMG copies that file
# and background.tiff in beside Hex.app and the Applications link.
# The .DS_Store finds the background by the volume name, Hex, and its path,
# /.background.tiff. Finder measures window_rect from the bottom of the screen.
files = [defines["app"]]
symlinks = {"Applications": "/Applications"}
background = defines["background"]
window_rect = ((200, 400), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 128
text_size = 13
icon_locations = {"Hex.app": (170, 178), "Applications": (490, 178)}
