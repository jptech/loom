# DSP pipeline project-specific constraints
# Additional timing constraints for the DSP datapath

# Input/output delays for AXI-Stream interface
set_input_delay  -clock sys_clk -max 3.0 [get_ports {data_in[*]}]
set_input_delay  -clock sys_clk -min 0.5 [get_ports {data_in[*]}]
set_output_delay -clock sys_clk -max 3.0 [get_ports {data_out[*]}]
set_output_delay -clock sys_clk -min 0.5 [get_ports {data_out[*]}]
