module top (
  input  logic clk,
  output logic out
);
  comp_a u_comp_a (.clk(clk), .out(out));
endmodule
