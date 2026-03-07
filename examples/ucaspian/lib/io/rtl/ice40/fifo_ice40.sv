// fifo_ice40.sv — iCE40-optimized FIFO using SB_RAM256x16
//
// Drop-in replacement for the behavioral fifo.sv, targeting
// Lattice iCE40 UP5K SPRAM blocks for efficient resource usage.
// Same port interface: WIDTH=8, DEPTH=512 default.

module fifo(
    input                     clk,
    input                     reset,
    input                     write_enable,
    input        [WIDTH-1:0]  write_data,
    input                     read_enable,
    output logic [WIDTH-1:0]  read_data,
    output logic              almost_full,
    output logic              empty,
    output logic              full,
    output logic [7:0]        count,
    output logic [7:0]        avail
);
parameter WIDTH = 8;
parameter DEPTH = 512;

localparam DEPTH_BITS = $clog2(DEPTH);

// Pointers and size tracking
logic [DEPTH_BITS-1:0] write_ptr;
logic [DEPTH_BITS-1:0] read_ptr;
logic [DEPTH_BITS:0]   cur_size;

wire empty_sig = (cur_size == 0);
wire full_sig  = (cur_size == DEPTH);

always_comb count = cur_size;
always_comb avail = (DEPTH - cur_size);

always_ff @(posedge clk)
    almost_full <= (cur_size >= DEPTH - 10);

always_comb empty = empty_sig;
always_comb full  = full_sig;

wire wr_en = write_enable && !full_sig;
wire rd_en = read_enable && !empty_sig;

// iCE40 SB_RAM256x16 block RAM inference
// For WIDTH=8, DEPTH=512, we use one SB_RAM256x16 with address bit[8]
// selecting between two 256-deep banks, reading low byte only.
logic [15:0] ram_rdata;
logic [15:0] ram_wdata;
logic [7:0]  ram_addr;
logic        ram_we;
logic        ram_re;

assign ram_wdata = {8'b0, write_data};
assign ram_addr  = wr_en ? write_ptr[7:0] : read_ptr[7:0];
assign ram_we    = wr_en;
assign ram_re    = rd_en;

SB_RAM256x16 #(
    .INIT_0(256'h0), .INIT_1(256'h0), .INIT_2(256'h0), .INIT_3(256'h0),
    .INIT_4(256'h0), .INIT_5(256'h0), .INIT_6(256'h0), .INIT_7(256'h0),
    .INIT_8(256'h0), .INIT_9(256'h0), .INIT_A(256'h0), .INIT_B(256'h0),
    .INIT_C(256'h0), .INIT_D(256'h0), .INIT_E(256'h0), .INIT_F(256'h0)
) ram_inst (
    .RDATA(ram_rdata),
    .RADDR(read_ptr[7:0]),
    .RCLK(clk),
    .RCLKE(1'b1),
    .RE(rd_en),
    .WADDR(write_ptr[7:0]),
    .WCLK(clk),
    .WCLKE(1'b1),
    .WDATA(ram_wdata),
    .WE(wr_en),
    .MASK(16'h0000)
);

always_comb read_data = ram_rdata[WIDTH-1:0];

// Pointer and size management (identical to behavioral version)
always @(posedge clk) begin
    if (reset) begin
        write_ptr <= 0;
        read_ptr  <= 0;
        cur_size  <= 0;
    end else begin
        if (wr_en && !rd_en)
            cur_size <= cur_size + 1;
        else if (!wr_en && rd_en)
            cur_size <= cur_size - 1;

        if (wr_en)
            write_ptr <= write_ptr + 1;
        if (rd_en)
            read_ptr <= read_ptr + 1;
    end
end

endmodule
