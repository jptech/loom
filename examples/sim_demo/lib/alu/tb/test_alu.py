# cocotb-based ALU test
#
# Demonstrates the runner = "cocotb" integration.
# Tests ADD and SUB operations with basic stimulus.

import cocotb
from cocotb.clock import Clock
from cocotb.triggers import RisingEdge, ClockCycles


@cocotb.test()
async def test_alu_add(dut):
    """Test ALU ADD operation."""
    clock = Clock(dut.clk, 10, unit="ns")
    cocotb.start_soon(clock.start())

    dut.rst_n.value = 0
    await ClockCycles(dut.clk, 3)
    dut.rst_n.value = 1
    await RisingEdge(dut.clk)

    # ADD: 0x10 + 0x20 = 0x30
    dut.a.value = 0x10
    dut.b.value = 0x20
    dut.op.value = 0b000
    await ClockCycles(dut.clk, 2)

    assert dut.result.value == 0x30, f"ADD failed: expected 0x30, got {dut.result.value:#x}"
    assert dut.zero_flag.value == 0, "zero_flag should be clear"


@cocotb.test()
async def test_alu_sub(dut):
    """Test ALU SUB operation."""
    clock = Clock(dut.clk, 10, unit="ns")
    cocotb.start_soon(clock.start())

    dut.rst_n.value = 0
    await ClockCycles(dut.clk, 3)
    dut.rst_n.value = 1
    await RisingEdge(dut.clk)

    # SUB: 0x50 - 0x20 = 0x30
    dut.a.value = 0x50
    dut.b.value = 0x20
    dut.op.value = 0b001
    await ClockCycles(dut.clk, 2)

    assert dut.result.value == 0x30, f"SUB failed: expected 0x30, got {dut.result.value:#x}"

    # SUB yielding zero: 0x42 - 0x42 = 0x00
    dut.a.value = 0x42
    dut.b.value = 0x42
    dut.op.value = 0b001
    await ClockCycles(dut.clk, 2)

    assert dut.result.value == 0x00, f"SUB-zero failed: expected 0x00, got {dut.result.value:#x}"
    assert dut.zero_flag.value == 1, "zero_flag should be set"
