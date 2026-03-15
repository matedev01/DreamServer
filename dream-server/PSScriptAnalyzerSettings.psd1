@{
    ExcludeRules = @(
        'PSAvoidUsingWriteHost'
        'PSAvoidUsingEmptyCatchBlock'
        'PSUseBOMForUnicodeEncodedFile'
        'PSUseShouldProcessForStateChangingFunctions'
        'PSUseApprovedVerbs'
        'PSUseDeclaredVarsMoreThanAssignments'
    )
    Rules = @{
        PSAvoidUsingConvertToSecureStringWithPlainText = @{
            Enable = $true
        }
    }
}
