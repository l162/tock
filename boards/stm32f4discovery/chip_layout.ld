/* Memory layout for the STM32F412G
 * rom = 1MB (LENGTH = 0x01000000)
 * kernel = 192KB
 * user = 832KB
 * ram = 256KB */

/* Layout for the stm32f3discovery board, usable by the examples in this repository. */

MEMORY
{
  rom (rx)  : ORIGIN = 0x08000000, LENGTH = 0x00080000
  prog (rx) : ORIGIN = 0x08080000, LENGTH = 0x00080000
  ram (rwx) : ORIGIN = 0x20000000, LENGTH = 0x00020000
}

MPU_MIN_ALIGN = 8K;
