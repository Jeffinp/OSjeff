# Scenario 2: viewer. corrupted image -> error; BMP/PNG/PPM; zoom, rotate, info.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 3
key down; sleep 0.4
key ret; sleep 1.5          # into Imagens (first row = corrompida.png)
shot s2-list
key ret; sleep 3            # open the corrupted image
shot s2-corrupt
key esc; sleep 1.5
key down; sleep 0.4         # foto.bmp
key ret; sleep 5
shot s2-bmp
key equal; sleep 1; key equal; sleep 1
shot s2-zoom
key 1; sleep 1
shot s2-actual
key 0; sleep 1; key r; sleep 2
key i; sleep 1
shot s2-rot-info
key right; sleep 4          # next image: foto.png
shot s2-png
key right; sleep 4          # foto.ppm
shot s2-ppm
key right; sleep 3          # pequena.png
key right; sleep 3          # transparente.png
key 0; sleep 1
shot s2-trans
finish
