import sys

with open('elf.rs', 'r') as f:
    lines = f.readlines()

# Find line 146 (is_user_range check) and insert validation after it
output = []
for i, line in enumerate(lines):
    output.append(line)
    # After the closing brace of the is_user_range check (around line 146)
    if i >= 144 and i <= 147 and line.strip() == '}':
        if i == 146:  # This should be the closing brace
            # Add guard page validation
            validation_code = '''        // Validate segment does not overlap with guard pages
        {
            use crate::memory::protection::{
                NULL_GUARD_SIZE, CODE_GUARD_VIRT, DATA_GUARD_VIRT,
                HEAP_GUARD_VIRT, STACK_GUARD_VIRT,
            };
            
            let guard_regions = [
                (0, NULL_GUARD_SIZE),
                (CODE_GUARD_VIRT, CODE_GUARD_VIRT + 4096),
                (DATA_GUARD_VIRT, DATA_GUARD_VIRT + 4096),
                (HEAP_GUARD_VIRT, HEAP_GUARD_VIRT + 4096),
                (STACK_GUARD_VIRT, STACK_GUARD_VIRT + 4096),
            ];
            
            for (guard_start, guard_end) in guard_regions.iter() {
                if p_vaddr < *guard_end && virt_end > *guard_start {
                    return Err(LoadError::UnsupportedSegmentLayout);
                }
            }
        }
'''
            output.append(validation_code)

with open('elf.rs', 'w') as f:
    f.writelines(output)

print("Guard page validation added")
