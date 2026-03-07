// native_bus_pkg.sv — Shared native bus interface types for register access
//
// Provides a simple request/response struct pair used internally by the
// register file and protocol wrappers.

package native_bus_pkg;

  typedef struct packed {
    logic [7:0]  addr;
    logic        wr;
    logic        rd;
    logic [31:0] wdata;
  } bus_req_t;

  typedef struct packed {
    logic [31:0] rdata;
    logic        ready;
  } bus_resp_t;

endpackage
