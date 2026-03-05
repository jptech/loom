module pwm(
	input clk,
	input [BITS-1:0] bright,
	output reg out
);
	parameter BITS = 8;

	reg [BITS-1:0] counter;
	always @(posedge clk)
	begin
		counter <= counter + 1;
		out <= counter < bright;
	end

endmodule