#!/usr/bin/env tclsh

set scriptDir [file dirname [file normalize [info script]]]

set inputFiles [list specification.md]
set outputFile requirements.tsv

################################################################################

# Proc definitions

# Scans $text for a requirement, beginning at $cursor. Sets $cursor to the
# index of the next character past the closing delimiter, or to -1 if there are
# no more requirements. Returns the text requirement, including the closing
# period.
proc scanRequirement {text cursorVarName} {
    upvar $cursorVarName cursor

    # Find the first carat character
    set cursor [string first {^} $text $cursor]
    if {$cursor == -1} {
        return ""
    }

    # Check if next character is an open parenthesis
    set cursor [expr {$cursor + 1}]
    set nextCharacter [string index $text $cursor]
    set isBracketed [expr {$nextCharacter eq {(}}]
    if {$isBracketed} {incr cursor}
    set requirementStartIndex $cursor

    # Find the indices of the end of the requirement and the new cursor position
    if {$isBracketed} {
        set delimiterStart [string first {)^} $text $requirementStartIndex]
        if {$delimiterStart < 0} {
            error "Unmatched closing delimiter for requirement starting at $requirementStartIndex"
        }
        set requirementEndIndex [expr {$delimiterStart - 1}]
        set cursor [expr {$delimiterStart + 2}]
    } else {
        set requirementEndIndex [string first {.} $text $requirementStartIndex]
        if {$requirementEndIndex < 0} {
            error "Unmatched closing delimiter for requirement starting at $requirementStartIndex"
        }
        set cursor [expr {$requirementEndIndex + 1}]
    }

    return [string range $text $requirementStartIndex $requirementEndIndex]
}

# Convert a text requirement to a tab-separated row with the following columns:
# checksumReadable - the checksum, in human-readable format
# checksumHex - the checksum, in hex string format
# requirement - the unaltered requirement
proc requirementToRow {requirement} {
    set checksumHex [md5Hex $requirement]
    set checksumBinary [binary format H* $checksumHex]
    set checksumReadable [binaryToReadable $checksumBinary]

    set columns [list $checksumReadable $checksumHex $requirement]
    return [join $columns \t]
}

# Returns the MD5 hash of $input in hex format, using md5sum
proc md5Hex {input} {
    # Get MD5 hash using md5sum command
    set hash [exec echo -n $input | md5sum]
    # Extract just the hex hash (md5sum outputs "hash  -")
    return [lindex $hash 0]
}

# Returns the input binary as a human-readable requirement string, as outlined
# in the requirements spec. (See about-reqs.md)
proc binaryToReadable {input} {
    set ints {}
    binary scan $input su8 ints
    set formattedInts [lmap int $ints {format "%05d" $int}]
    return [format "R-%s" [join $formattedInts -]]
}

################################################################################

# Main execution

set requirements {}
foreach file $inputFiles {
    set inputFilePath [file join $scriptDir $file]
    set fd [open $inputFilePath]
    set fileContents [read $fd]
    close $fd

    set cursor 0
    while {$cursor >= 0} {
        set requirement [scanRequirement $fileContents cursor]
        if {[string trim $requirement] != ""} {
            lappend requirements $requirement
        }
    }
}

set requirementCount [llength $requirements]
puts "Writing $requirementCount requirements to $outputFile"

set outputFilePath [file join $scriptDir $outputFile]
set rows [lmap requirement $requirements {requirementToRow $requirement}]

set fd [open $outputFilePath w]
puts $fd [join $rows \n]
close $fd
