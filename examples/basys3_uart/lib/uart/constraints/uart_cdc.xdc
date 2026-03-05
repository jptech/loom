# CDC false path for 2FF synchronizer
# Targets the first register stage of any cdc_sync instance
set_false_path -to [get_cells -hierarchical -filter {NAME =~ *cdc*sync_0_reg*}]

# Max delay on CDC data bus (one UART clock period at 50 MHz)
set_max_delay -datapath_only 20.0 \
    -from [get_cells -hierarchical -filter {NAME =~ *uart_rx*rx_data_reg*}] \
    -to [get_cells -hierarchical -filter {NAME =~ *rx_data_latched_reg*}]
