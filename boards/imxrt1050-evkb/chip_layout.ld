/* Memory layout for the i.MX RT 1050 EVKB
 * rom = 2MB (LENGTH = 0x02000000)
 * kernel = 256KB
 * user = 256KB
 * ram = 192KB */

MEMORY
{
  hdr (rx)  : ORIGIN = 0x60000000, LENGTH = 0x2000
  rom (rx)  : ORIGIN = 0x60002000, LENGTH = 0x2FFE000
  prog (rx) : ORIGIN = 0x63000000, LENGTH = 0x1000000
  ram (rwx) : ORIGIN = 0x20000000, LENGTH = 0x20000
}

MPU_MIN_ALIGN = 8K;


SECTIONS {
  .boot_hdr  : ALIGN(4)
    {
        KEEP (*(.boot_hdr))
    } > hdr
    
}