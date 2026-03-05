# Component-scoped timing constraint for blinky
# Applied with -ref scoping in Vivado non-project mode

# False path on LED outputs (no timing requirement on slow-toggle signals)
set_false_path -to [get_ports led[*]]
