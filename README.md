# mouse-configurator-fedora
Configurator for HP 930 Creator Wireless Mouse

Compiling this on Fedora 37, you will need to install the following:

$ dnf install cargo rust-libudev* rust-gtk4-sys*

Then:

$ sudo make install

You should now be able to run: mouse-configurator

Notes:

The only modification I've done to the original code is modify the bindings to move around virtual desktops in GNOME on Fedora. I've also added this README
