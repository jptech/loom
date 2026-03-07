# Numato Mimas A7 — XC7A50T-1FGG484C
# Pin assignments from NumatoLab VivadoBSP
# https://github.com/NumatoLab/VivadoBSP/tree/main/MimasA7/1.0
#
# uCaspian neuromorphic processor — minimal pin set

# 100 MHz oscillator
set_property -dict {PACKAGE_PIN H4 IOSTANDARD LVCMOS33} [get_ports clk]
create_clock -period 10.000 -name sys_clk [get_ports clk]

# Reset push button (active high)
set_property -dict {PACKAGE_PIN M2 IOSTANDARD LVCMOS33} [get_ports reset]

# USB-UART bridge
set_property -dict {PACKAGE_PIN Y21 IOSTANDARD LVCMOS33} [get_ports usb_uart_txd]
set_property -dict {PACKAGE_PIN Y22 IOSTANDARD LVCMOS33} [get_ports usb_uart_rxd]

# MMCM generated clock — let Vivado derive timing automatically
# (no need to constrain clk_sys; Vivado infers it from the MMCM output)
