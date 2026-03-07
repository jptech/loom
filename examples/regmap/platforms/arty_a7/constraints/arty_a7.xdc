# Digilent Arty A7-35T
# Clock
set_property -dict {PACKAGE_PIN E3 IOSTANDARD LVCMOS33} [get_ports clk]
create_clock -period 10.000 -name sys_clk [get_ports clk]

# Reset (active-low button BTN0)
set_property -dict {PACKAGE_PIN D9 IOSTANDARD LVCMOS33} [get_ports rst_n]

# GPIO — directly mapped to on-board LEDs and switches
set_property -dict {PACKAGE_PIN H5  IOSTANDARD LVCMOS33} [get_ports {gpio_io[0]}]
set_property -dict {PACKAGE_PIN J5  IOSTANDARD LVCMOS33} [get_ports {gpio_io[1]}]
set_property -dict {PACKAGE_PIN T9  IOSTANDARD LVCMOS33} [get_ports {gpio_io[2]}]
set_property -dict {PACKAGE_PIN T10 IOSTANDARD LVCMOS33} [get_ports {gpio_io[3]}]

# IRQ output (active-high, directly drives LED4)
set_property -dict {PACKAGE_PIN H6 IOSTANDARD LVCMOS33} [get_ports irq]
