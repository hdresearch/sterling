# templates/python/models.py.template
from typing import Optional, List, Dict, Any, Union
from pydantic import BaseModel, Field
from datetime import datetime
from enum import Enum

{{#enums}}
class {{name}}(str, Enum):
    """{{description}}"""
    {{#values}}
    {{name}} = "{{value}}"
    {{/values}}
{{/enums}}

{{#models}}
class {{name}}(BaseModel):
    """{{description}}"""
    {{#properties}}
    {{name}}: {{#optional}}Optional[{{/optional}}{{type}}{{#optional}}]{{/optional}}{{#has_default}} = {{default}}{{/has_default}}{{#has_field_info}} = Field({{field_info}}){{/has_field_info}}
    {{/properties}}
    
    class Config:
        extra = "forbid"
        use_enum_values = True
        validate_assignment = True
        {{#has_examples}}
        schema_extra = {
            "example": {{example}}
        }
        {{/has_examples}}
{{/models}}
