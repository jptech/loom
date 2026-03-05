# 100 MHz oscillator
set_property -dict {PACKAGE_PIN W5 IOSTANDARD LVCMOS33} [get_ports clk]
create_clock -period 10.000 -name sys_clk [get_ports clk]

# Reset button (center)
set_property -dict {PACKAGE_PIN U18 IOSTANDARD LVCMOS33} [get_ports rst_n]

# UART (USB-UART bridge on Basys 3)
set_property -dict {PACKAGE_PIN A18 IOSTANDARD LVCMOS33} [get_ports uart_txd]
set_property -dict {PACKAGE_PIN B18 IOSTANDARD LVCMOS33} [get_ports uart_rxd]

# 7-segment cathodes (active-low)
set_property -dict {PACKAGE_PIN W7  IOSTANDARD LVCMOS33} [get_ports {seg[0]}]
set_property -dict {PACKAGE_PIN W6  IOSTANDARD LVCMOS33} [get_ports {seg[1]}]
set_property -dict {PACKAGE_PIN U8  IOSTANDARD LVCMOS33} [get_ports {seg[2]}]
set_property -dict {PACKAGE_PIN V8  IOSTANDARD LVCMOS33} [get_ports {seg[3]}]
set_property -dict {PACKAGE_PIN U5  IOSTANDARD LVCMOS33} [get_ports {seg[4]}]
set_property -dict {PACKAGE_PIN V5  IOSTANDARD LVCMOS33} [get_ports {seg[5]}]
set_property -dict {PACKAGE_PIN U7  IOSTANDARD LVCMOS33} [get_ports {seg[6]}]

# 7-segment anodes (active-low)
set_property -dict {PACKAGE_PIN U2  IOSTANDARD LVCMOS33} [get_ports {an[0]}]
set_property -dict {PACKAGE_PIN U4  IOSTANDARD LVCMOS33} [get_ports {an[1]}]
set_property -dict {PACKAGE_PIN V4  IOSTANDARD LVCMOS33} [get_ports {an[2]}]
set_property -dict {PACKAGE_PIN W4  IOSTANDARD LVCMOS33} [get_ports {an[3]}]

# LED (heartbeat indicator)
set_property -dict {PACKAGE_PIN U16 IOSTANDARD LVCMOS33} [get_ports led_heartbeat]
