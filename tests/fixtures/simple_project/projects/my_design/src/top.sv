import axi_common_pkg::*;

module top (
  input  logic clk,
  input  logic rst_n
);
  // Minimal top module for test fixture
  logic [1:0] resp;
  assign resp = AXI_RESP_OKAY;
endmodule
