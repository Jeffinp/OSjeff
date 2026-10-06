source "$(dirname "$0")/../lib.sh"
wait_first_frame
for r in 1 2 3; do for k in h e l p ret; do key $k; sleep 0.25; done; done
for r in 1 2 3 4 5 6; do for k in a b c d e f g h i j k l m n o p q r s t u v w x y z; do key $k; sleep 0.15; done; done
sleep 1
finish
