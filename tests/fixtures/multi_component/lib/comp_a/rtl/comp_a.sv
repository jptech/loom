module comp_a (
  input  logic clk,
  output logic out
);
  comp_b u_comp_b (.clk(clk), .out(out));
endmodule
