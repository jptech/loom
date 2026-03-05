# MMCM reset is asynchronous — exclude from timing analysis
set_false_path -to [get_pins mmcme2_inst/RST]
