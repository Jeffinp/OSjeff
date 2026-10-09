# The same window minimised: a hidden Tarefas costs nothing beyond the sampler.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon tasks; click; sleep 3
goto 228 68; click; sleep 22     # the yellow light of the window at (190,52)
finish
