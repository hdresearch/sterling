-- migrate:up

CREATE TABLE labels (
    label_id        BIGSERIAL PRIMARY KEY,
    vm_id           UUID NOT NULL REFERENCES vms(vm_id) ON DELETE CASCADE,
    label_name      VARCHAR(255) NOT NULL,
    label_value     VARCHAR(255) NOT NULL,
    UNIQUE (vm_id, label_name)
);



CREATE INDEX labels_vm_id_idx ON labels (vm_id);

-- migrate:down

DROP TABLE labels;
